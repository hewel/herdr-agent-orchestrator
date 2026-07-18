//! Coordinator-owned explicit Worker launch profile registry.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::contract::{HarnessId, HarnessKind, HarnessLaunchProfileV1, Validate};

/// Exact profile selection retained with a Harness Session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLaunchProfile {
    /// Parsed, validated public profile.
    pub profile: HarnessLaunchProfileV1,
    /// Exact source file contents.
    pub snapshot: String,
    /// Lowercase SHA-256 of [`Self::snapshot`].
    pub digest: String,
    /// Explicitly inherited environment values that were present.
    pub environment: BTreeMap<String, String>,
}

/// Profile discovery or explicit-resolution failure.
#[derive(Debug, Error)]
pub enum ProfileError {
    /// Profile directory or file could not be read.
    #[error("cannot read launch profile path `{path}`: {source}")]
    Io {
        /// Failed path.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// TOML did not decode to the v1 profile contract.
    #[error("invalid launch profile TOML `{path}`: {source}")]
    Toml {
        /// Invalid file.
        path: PathBuf,
        /// TOML decoder error.
        source: toml::de::Error,
    },
    /// Typed profile validation failed.
    #[error("invalid launch profile `{path}`: {message}")]
    Validation {
        /// Invalid file.
        path: PathBuf,
        /// Contract failure.
        message: String,
    },
    /// Two files declared the same durable profile ID.
    #[error("duplicate launch profile ID `{0}`")]
    Duplicate(HarnessId),
    /// Caller selected no registered profile with this ID.
    #[error("launch profile `{0}` does not exist")]
    NotFound(HarnessId),
    /// Explicit Worker Kind differs from the selected profile.
    #[error("launch profile `{profile}` is not compatible with {actual:?}")]
    KindMismatch {
        /// Selected profile.
        profile: HarnessId,
        /// Requested Harness Kind.
        actual: HarnessKind,
    },
    /// Referenced executable or overlay is not a regular file.
    #[error("launch profile `{profile}` references a missing or non-regular file `{path}`")]
    MissingFile {
        /// Selected profile.
        profile: HarnessId,
        /// Invalid file reference.
        path: PathBuf,
    },
}

#[derive(Debug, Clone)]
struct RegistryEntry {
    profile: HarnessLaunchProfileV1,
    snapshot: String,
    digest: String,
}

/// Immutable in-memory registry loaded from Coordinator-owned TOML files.
#[derive(Debug, Clone, Default)]
pub struct ProfileRegistry {
    entries: BTreeMap<HarnessId, RegistryEntry>,
}

impl ProfileRegistry {
    /// Loads every direct `*.toml` child in lexical path order.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError`] for unreadable, invalid, duplicate, or unsafe profiles.
    pub fn load(directory: &Path) -> Result<Self, ProfileError> {
        let read = fs::read_dir(directory).map_err(|source| ProfileError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let mut paths = read
            .map(|entry| {
                entry
                    .map(|value| value.path())
                    .map_err(|source| ProfileError::Io {
                        path: directory.to_path_buf(),
                        source,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        paths.retain(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        });
        paths.sort();
        let mut entries = BTreeMap::new();
        for path in paths {
            let snapshot = fs::read_to_string(&path).map_err(|source| ProfileError::Io {
                path: path.clone(),
                source,
            })?;
            let profile: HarnessLaunchProfileV1 =
                toml::from_str(&snapshot).map_err(|source| ProfileError::Toml {
                    path: path.clone(),
                    source,
                })?;
            profile
                .validate()
                .map_err(|error| ProfileError::Validation {
                    path: path.clone(),
                    message: error.to_string(),
                })?;
            validate_files(&profile)?;
            let id = profile.id.clone();
            let digest = hex::encode(Sha256::digest(snapshot.as_bytes()));
            if entries
                .insert(
                    id.clone(),
                    RegistryEntry {
                        profile,
                        snapshot,
                        digest,
                    },
                )
                .is_some()
            {
                return Err(ProfileError::Duplicate(id));
            }
        }
        Ok(Self { entries })
    }

    /// Lists registered IDs without selecting or ranking one.
    #[must_use]
    pub fn ids(&self) -> Vec<HarnessId> {
        self.entries.keys().cloned().collect()
    }

    /// Resolves exactly the caller-selected ID and Kind.
    ///
    /// Environment is filtered to names explicitly declared by the profile.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError`] when the profile is absent or has another Kind.
    pub fn resolve<I, K, V>(
        &self,
        id: &HarnessId,
        kind: HarnessKind,
        environment: I,
    ) -> Result<ResolvedLaunchProfile, ProfileError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let entry = self
            .entries
            .get(id)
            .ok_or_else(|| ProfileError::NotFound(id.clone()))?;
        if entry.profile.kind != kind {
            return Err(ProfileError::KindMismatch {
                profile: id.clone(),
                actual: kind,
            });
        }
        let allow = entry
            .profile
            .inherit_env
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let environment = environment
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .filter(|(key, _)| allow.contains(key))
            .collect();
        Ok(ResolvedLaunchProfile {
            profile: entry.profile.clone(),
            snapshot: entry.snapshot.clone(),
            digest: entry.digest.clone(),
            environment,
        })
    }
}

fn validate_files(profile: &HarnessLaunchProfileV1) -> Result<(), ProfileError> {
    for path in std::iter::once(&profile.executable).chain(&profile.config_overlays) {
        if !fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            return Err(ProfileError::MissingFile {
                profile: profile.id.clone(),
                path: path.clone(),
            });
        }
    }
    Ok(())
}
