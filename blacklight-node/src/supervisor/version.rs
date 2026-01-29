use anyhow::{Result, anyhow};
use semver::Version;

use blacklight_contract_clients::BlacklightClient;
use tracing::{debug, error, info, warn};

/// BLACKLIGHT_VERSION is injected at build time by CI, falls back to Cargo.toml version for local builds
pub const VERSION: &str = match option_env!("BLACKLIGHT_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => env!("CARGO_PKG_VERSION"),
};

/// Result of semantic version comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionCompatibility {
    /// SC Version and Node Version are exactly equal
    Equal,
    /// Node Version is newer than SC Version but compatible
    NewerCompatible,
    /// Node Version is older than SC Version but compatible
    OlderCompatible,
    /// Node Version and SC Version are incompatible
    Incompatible,
}

/// Compare node version against required version using semantic versioning.
///
/// Compatibility rules:
/// - For 0.x.y versions (unstable API): minor versions must match exactly
///   (0.8.0 and 0.9.0 are incompatible, but 0.9.0 and 0.9.1 are compatible)
/// - For 1.0.0+: major versions must match
///   (1.2.0 and 2.0.0 are incompatible, but 1.2.0 and 1.3.0 are compatible)
/// - Within a compatibility group, older versions trigger a warning but are allowed
pub fn check_version_compatibility(
    node_version: &str,
    required_version: &str,
) -> Result<VersionCompatibility> {
    let node = Version::parse(node_version)
        .map_err(|e| anyhow!("Invalid node version '{}': {}", node_version, e))?;
    let required = Version::parse(required_version)
        .map_err(|e| anyhow!("Invalid required version '{}': {}", required_version, e))?;

    if node == required {
        return Ok(VersionCompatibility::Equal);
    }

    // Check if versions are in the same compatibility group:
    // - 0.x: same major (0) AND same minor
    // - 1.x+: same major
    let in_same_group = if required.major == 0 {
        node.major == 0 && node.minor == required.minor
    } else {
        node.major == required.major
    };

    if !in_same_group {
        return Ok(VersionCompatibility::Incompatible);
    }

    // In same compatibility group - classify by direction
    if node > required {
        Ok(VersionCompatibility::NewerCompatible)
    } else {
        Ok(VersionCompatibility::OlderCompatible)
    }
}

/// Validate node version against the protocol's required version
pub async fn validate_node_version(client: &BlacklightClient) -> Result<()> {
    let required_version = client.protocol_config.node_version().await?;
    let required_version = required_version.trim();

    // Empty required version means no version enforcement
    if required_version.is_empty() {
        info!(
            node_version = VERSION,
            "Node version (no protocol requirement)"
        );
        return Ok(());
    }

    let compatibility = check_version_compatibility(VERSION, required_version)?;

    let upgrade_cmd = format!(
        "docker pull ghcr.io/nillionnetwork/blacklight-node/blacklight_node:{}",
        required_version
    );

    match compatibility {
        VersionCompatibility::Equal => {
            debug!(
                node_version = VERSION,
                required_version, "Node version matches protocol requirement"
            );
            Ok(())
        }

        VersionCompatibility::NewerCompatible => {
            info!(
                node_version = VERSION,
                required_version, "Node version is newer and compatible with protocol requirement"
            );
            Ok(())
        }

        VersionCompatibility::OlderCompatible => {
            warn!(
                node_version = VERSION,
                required_version,
                upgrade_cmd = %upgrade_cmd,
                "Node version is older than recommended; consider upgrading"
            );
            Ok(())
        }

        VersionCompatibility::Incompatible => {
            error!(
                node_version = VERSION,
                required_version,
                upgrade_cmd = %upgrade_cmd,
                "Node version is incompatible with protocol requirement; upgrade required"
            );

            Err(anyhow!(
                "Node version {} is incompatible with required {}. Upgrade with: {}",
                VERSION,
                required_version,
                upgrade_cmd
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_equal_versions() {
        assert_eq!(
            check_version_compatibility("1.2.3", "1.2.3").unwrap(),
            VersionCompatibility::Equal
        );
    }

    #[test]
    fn test_newer_compatible_newer_minor() {
        assert_eq!(
            check_version_compatibility("1.3.0", "1.2.0").unwrap(),
            VersionCompatibility::NewerCompatible
        );
    }

    #[test]
    fn test_newer_compatible_newer_patch() {
        assert_eq!(
            check_version_compatibility("1.2.5", "1.2.3").unwrap(),
            VersionCompatibility::NewerCompatible
        );
    }

    #[test]
    fn test_older_compatible_minor() {
        // Same major (1.x), older minor is allowed but triggers warning
        assert_eq!(
            check_version_compatibility("1.1.0", "1.2.0").unwrap(),
            VersionCompatibility::OlderCompatible,
        );
    }

    #[test]
    fn test_older_compatible_patch() {
        // Same major (1.x), older patch is allowed but triggers warning
        assert_eq!(
            check_version_compatibility("1.2.1", "1.2.3").unwrap(),
            VersionCompatibility::OlderCompatible,
        );
    }

    #[test]
    fn test_incompatible_major() {
        assert_eq!(
            check_version_compatibility("2.0.0", "1.2.3").unwrap(),
            VersionCompatibility::Incompatible
        );
        assert_eq!(
            check_version_compatibility("1.0.0", "2.0.0").unwrap(),
            VersionCompatibility::Incompatible
        );
    }

    #[test]
    fn test_older_compatible_patch_2() {
        assert_eq!(
            check_version_compatibility("1.2.3", "1.2.5").unwrap(),
            VersionCompatibility::OlderCompatible
        );
        assert_eq!(
            check_version_compatibility("1.3.0", "1.4.0").unwrap(),
            VersionCompatibility::OlderCompatible
        );
    }
    #[test]
    fn test_invalid_version() {
        assert!(check_version_compatibility("invalid", "1.2.3").is_err());
        assert!(check_version_compatibility("1.2.3", "invalid").is_err());
    }

    // 0.x version tests - minor version changes are breaking
    #[test]
    fn test_zero_major_minor_mismatch_incompatible() {
        assert_eq!(
            check_version_compatibility("0.8.0", "0.9.0").unwrap(),
            VersionCompatibility::Incompatible
        );
        assert_eq!(
            check_version_compatibility("0.9.0", "0.8.0").unwrap(),
            VersionCompatibility::Incompatible
        );
    }

    #[test]
    fn test_zero_major_same_minor_newer_compatible() {
        assert_eq!(
            check_version_compatibility("0.9.1", "0.9.0").unwrap(),
            VersionCompatibility::NewerCompatible
        );
    }

    #[test]
    fn test_zero_major_same_minor_older_compatible() {
        // Same minor (0.9.x), older patch is allowed but triggers warning
        assert_eq!(
            check_version_compatibility("0.9.0", "0.9.1").unwrap(),
            VersionCompatibility::OlderCompatible
        );
    }

    #[test]
    fn test_zero_major_equal() {
        assert_eq!(
            check_version_compatibility("0.9.0", "0.9.0").unwrap(),
            VersionCompatibility::Equal
        );
    }
    #[test]
    fn test_package_version() {
        assert_eq!("0.9.0", env!("CARGO_PKG_VERSION"));
    }
}
