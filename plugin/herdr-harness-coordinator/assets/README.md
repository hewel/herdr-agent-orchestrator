# Harness Coordinator plugin assets

This directory is reserved for static popup assets. The MVP popup is rendered
by the `herdr-harness-coordinator popup` process and queries durable Coordinator
state; it does not screen-scrape panes or own any Harness lifecycle.

The `worker` entrypoint inherits `HERDR_SOCKET_PATH` and
`HERDR_PLUGIN_STATE_DIR` from Herdr and receives
`HERDR_HARNESS_SESSION_ID` from `plugin.pane.open`.
