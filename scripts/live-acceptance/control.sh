#!/bin/sh
set -eu

usage() {
  cat >&2 <<'EOF'
usage: control.sh <workspace-state-dir> <command> [arguments]

commands:
  status                         list durable Tasks
  graph                          show the durable Task graph
  inbox                          show the Supervisor inbox
  events                         show durable Supervisor Events
  start <worker-id>              start or reconnect a selected Worker
  handoff <candidate-binary>     restart the daemon on a tested binary
  call <tool-name> [json|-]       call any Coordinator MCP tool (`-` reads stdin)
  evidence [output-file]         capture the durable acceptance evidence

HERDR_COORDINATOR_BIN and HERDR_COORDINATOR_SOCKET override auto-discovery.
EOF
  exit 2
}

[ "$#" -ge 2 ] || usage

state_dir=${1%/}
command=$2
shift 2

[ -d "$state_dir" ] || {
  echo "workspace state directory does not exist: $state_dir" >&2
  exit 1
}

capability_file=$state_dir/supervisor.capability
[ -r "$capability_file" ] || {
  echo "Supervisor capability is not readable: $capability_file" >&2
  exit 1
}
supervisor_capability=$(sed -n '1p' "$capability_file")

validate_daemon_pid() {
  daemon_pid=$1
  expected_executable=${2:-}
  kill -0 "$daemon_pid" 2>/dev/null || return 1
  daemon_executable=$(readlink -f "/proc/$daemon_pid/exe") || return 1
  [ "$(basename "$daemon_executable")" = "herdr-harness-coordinator" ] || return 1
  if [ -n "$expected_executable" ]; then
    [ "$daemon_executable" = "$expected_executable" ] || return 1
  fi
  daemon_argv=$(tr '\0' '\n' <"/proc/$daemon_pid/cmdline") || return 1
  printf '%s\n' "$daemon_argv" | rg -F -x 'daemon' >/dev/null || return 1
  printf '%s\n' "$daemon_argv" | rg -F -x "$state_dir" >/dev/null || return 1
}

resolve_binary() {
  if [ -n "${HERDR_COORDINATOR_BIN:-}" ]; then
    printf '%s\n' "$HERDR_COORDINATOR_BIN"
    return
  fi
  if [ -r "$state_dir/coordinator.bin" ]; then
    sed -n '1p' "$state_dir/coordinator.bin"
    return
  fi
  command -v herdr-harness-coordinator
}

resolve_socket() {
  if [ -n "${HERDR_COORDINATOR_SOCKET:-}" ]; then
    printf '%s\n' "$HERDR_COORDINATOR_SOCKET"
    return
  fi
  digest=$(basename "$state_dir")
  plugin_state=$(dirname "$(dirname "$state_dir")")
  printf '%s/s/%.24s.sock\n' "$plugin_state" "$digest"
}

call_tool() {
  tool_name=$1
  if [ "$#" -eq 2 ]; then
    arguments=$2
  else
    arguments='{}'
  fi
  printf '%s' "$arguments" | jq -e 'type == "object"' >/dev/null

  request=$(jq -cn \
    --arg name "$tool_name" \
    --argjson arguments "$arguments" \
    '{jsonrpc:"2.0",id:"live-control",method:"tools/call",params:{name:$name,arguments:$arguments}}')

  printf '%s\n' "$request" | "$coordinator_bin" mcp \
    --session-capability "$supervisor_capability" \
    --state-dir "$state_dir" \
    --socket "$coordinator_socket" | jq .
}

case "$command" in
  handoff)
    [ "$#" -eq 1 ] || usage
    candidate=$(readlink -f "$1")
    [ -x "$candidate" ] || {
      echo "candidate binary is not executable: $candidate" >&2
      exit 1
    }
    pid_file=$state_dir/coordinator.pid
    [ -r "$pid_file" ] || {
      echo "Coordinator PID file is not readable: $pid_file" >&2
      exit 1
    }
    old_pid=$(sed -n '1p' "$pid_file")
    if kill -0 "$old_pid" 2>/dev/null; then
      validate_daemon_pid "$old_pid" || {
        echo "PID file does not identify this workspace's Coordinator daemon: $old_pid" >&2
        exit 1
      }
      kill -INT "$old_pid"
      attempts=0
      while kill -0 "$old_pid" 2>/dev/null; do
        attempts=$((attempts + 1))
        [ "$attempts" -lt 100 ] || {
          echo "Coordinator did not stop after SIGINT: $old_pid" >&2
          exit 1
        }
        sleep 0.1
      done
    fi
    plugin_state=$(dirname "$(dirname "$state_dir")")
    activation=$($candidate workspace --state-dir "$plugin_state" list --json)
    workspace_id=$(printf '%s' "$activation" | jq -er --arg state "$state_dir" \
      '.[] | select(.state_dir == $state) | .workspace_id')
    repository_root=$(printf '%s' "$activation" | jq -er --arg state "$state_dir" \
      '.[] | select(.state_dir == $state) | .repository_root')
    session_socket=${HERDR_SOCKET_PATH:?handoff must run inside Herdr or set HERDR_SOCKET_PATH}
    set +e
    activation_output=$($candidate workspace --state-dir "$plugin_state" set on \
      --workspace "$workspace_id" --root "$repository_root" \
      --session-socket "$session_socket" --json 2>&1)
    activation_status=$?
    set -e
    coordinator_socket=$(resolve_socket)
    [ -S "$coordinator_socket" ] || {
      printf '%s\n' "$activation_output" >&2
      echo "candidate Coordinator did not create its socket" >&2
      exit 1
    }
    new_pid=$(sed -n '1p' "$pid_file")
    validate_daemon_pid "$new_pid" "$candidate" || {
      echo "activation did not launch the expected candidate daemon" >&2
      exit 1
    }
    health_request=$(jq -cn \
      '{jsonrpc:"2.0",id:"handoff-health",method:"tools/call",params:{name:"harness_status",arguments:{}}}')
    health_response=$(printf '%s\n' "$health_request" | "$candidate" mcp \
      --session-capability "$supervisor_capability" \
      --state-dir "$state_dir" --socket "$coordinator_socket")
    printf '%s' "$health_response" | jq -e \
      '.error == null and .result != null and .result.isError == false' >/dev/null || {
      printf '%s\n' "$health_response" >&2
      echo "candidate daemon failed its authenticated broker health query" >&2
      exit 1
    }
    printf '%s' "$candidate" >"$state_dir/coordinator.bin"
    if [ "$activation_status" -ne 0 ]; then
      printf 'workspace recovery warning: %s\n' "$activation_output" >&2
    fi
    printf 'Coordinator handoff: %s -> %s (%s)\n' "$old_pid" "$new_pid" "$candidate"
    exit 0
    ;;
  evidence)
    [ "$#" -le 1 ] || usage
    script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    if [ "$#" -eq 1 ]; then
      "$script_dir/capture-evidence.sh" "$state_dir" >"$1"
      printf 'evidence written to %s\n' "$1"
    else
      "$script_dir/capture-evidence.sh" "$state_dir"
    fi
    exit 0
    ;;
esac

coordinator_bin=$(resolve_binary)
[ -x "$coordinator_bin" ] || {
  echo "Coordinator binary is not executable: $coordinator_bin" >&2
  exit 1
}
coordinator_socket=$(resolve_socket)
[ -S "$coordinator_socket" ] || {
  echo "Coordinator socket is not live: $coordinator_socket" >&2
  exit 1
}
case "$command" in
  status) [ "$#" -eq 0 ] || usage; call_tool harness_status ;;
  graph) [ "$#" -eq 0 ] || usage; call_tool harness_task_graph ;;
  inbox) [ "$#" -eq 0 ] || usage; call_tool harness_inbox ;;
  events) [ "$#" -eq 0 ] || usage; call_tool harness_supervisor_events ;;
  start)
    [ "$#" -eq 1 ] || usage
    call_tool harness_start "$(jq -cn --arg worker_id "$1" '{worker_id:$worker_id}')"
    ;;
  call)
    [ "$#" -ge 1 ] && [ "$#" -le 2 ] || usage
    if [ "$#" -eq 2 ]; then
      if [ "$2" = "-" ]; then
        call_tool "$1" "$(sed -n '1,$p')"
      else
        call_tool "$1" "$2"
      fi
    else
      call_tool "$1"
    fi
    ;;
  *) usage ;;
esac
