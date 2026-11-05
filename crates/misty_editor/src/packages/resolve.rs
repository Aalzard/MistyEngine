//! Dependency resolution: pick one version of every package the project needs (directly or
//! through other packages) that satisfies every range asking for it, preferring the newest.
//! Packages in the project folder (local / git) are pinned to the version they have. When no
//! version fits, the package is reported as a conflict with every requirement and who made it.

use std::collections::BTreeMap;

use super::semver::{Range, Version};

/// Where a candidate version comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    BuiltIn,
    /// A folder under `Packages/` (installed from disk, a zip or git).
    Installed,
    Registry,
}

/// One version of a package that could be used.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub version: Version,
    pub origin: Origin,
    pub dependencies: BTreeMap<String, String>,
}

/// One requirement on a package: who asked and for what range.
#[derive(Clone, Debug, PartialEq)]
pub struct Requirement {
    /// "project" or the requiring package's name.
    pub from: String,
    pub range: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    pub name: String,
    pub requirements: Vec<Requirement>,
    /// Versions that exist (none satisfies every requirement).
    pub available: Vec<Version>,
}

impl Conflict {
    pub fn message(&self) -> String {
        let reqs: Vec<String> = self
            .requirements
            .iter()
            .map(|r| format!("{} needs {}", r.from, r.range))
            .collect();
        let avail = if self.available.is_empty() {
            "no version is available".to_owned()
        } else {
            let v: Vec<String> = self.available.iter().map(|v| v.to_string()).collect();
            format!("available: {}", v.join(", "))
        };
        format!("{}: {} ({avail})", self.name, reqs.join("; "))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub version: Version,
    pub origin: Origin,
    pub dependencies: BTreeMap<String, String>,
    /// Not in the manifest: pulled in by another package.
    pub indirect: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolution {
    pub packages: BTreeMap<String, Resolved>,
    pub conflicts: Vec<Conflict>,
}

/// Resolve `roots` (manifest name → range) against `available` candidates.
pub fn resolve(
    roots: &BTreeMap<String, String>,
    available: &BTreeMap<String, Vec<Candidate>>,
) -> Resolution {
    let mut selected: BTreeMap<String, Candidate> = BTreeMap::new();
    let mut conflicts: BTreeMap<String, Conflict> = BTreeMap::new();
    for _ in 0..64 {
        // Every requirement from the project and the packages chosen so far.
        let mut reqs: BTreeMap<String, Vec<Requirement>> = BTreeMap::new();
        for (name, range) in roots {
            reqs.entry(name.clone()).or_default().push(Requirement {
                from: "project".into(),
                range: range.clone(),
            });
        }
        for (owner, c) in &selected {
            for (name, range) in &c.dependencies {
                reqs.entry(name.clone()).or_default().push(Requirement {
                    from: owner.clone(),
                    range: range.clone(),
                });
            }
        }
        let mut next: BTreeMap<String, Candidate> = BTreeMap::new();
        conflicts.clear();
        for (name, list) in &reqs {
            let ranges: Vec<Range> = list
                .iter()
                .map(|r| Range::parse(&r.range).unwrap_or_else(|_| Range::any()))
                .collect();
            let cands = available.get(name).map(Vec::as_slice).unwrap_or(&[]);
            // A package in the project folder is the only version that counts.
            let pinned: Vec<&Candidate> = cands
                .iter()
                .filter(|c| c.origin != Origin::Registry)
                .collect();
            let pool: Vec<&Candidate> = if pinned.is_empty() {
                cands.iter().collect()
            } else {
                pinned
            };
            let best = pool
                .iter()
                .filter(|c| ranges.iter().all(|r| r.matches(&c.version)))
                .max_by(|a, b| a.version.cmp(&b.version));
            match best {
                Some(c) => {
                    next.insert(name.clone(), (*c).clone());
                }
                None => {
                    let mut available: Vec<Version> =
                        pool.iter().map(|c| c.version.clone()).collect();
                    available.sort();
                    available.dedup();
                    conflicts.insert(
                        name.clone(),
                        Conflict {
                            name: name.clone(),
                            requirements: list.clone(),
                            available,
                        },
                    );
                }
            }
        }
        if next == selected {
            break;
        }
        selected = next;
    }
    Resolution {
        packages: selected
            .into_iter()
            .map(|(name, c)| {
                let indirect = !roots.contains_key(&name);
                (
                    name,
                    Resolved {
                        version: c.version,
                        origin: c.origin,
                        dependencies: c.dependencies,
                        indirect,
                    },
                )
            })
            .collect(),
        conflicts: conflicts.into_values().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(v: &str, origin: Origin, deps: &[(&str, &str)]) -> Candidate {
        Candidate {
            version: Version::parse(v).unwrap(),
            origin,
            dependencies: deps
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        }
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn picks_the_newest_version_every_range_allows() {
        let mut av = BTreeMap::new();
        av.insert(
            "com.a".to_owned(),
            vec![
                cand("1.0.0", Origin::Registry, &[("com.b", "^1.1.0")]),
                cand("1.2.0", Origin::Registry, &[("com.b", "^1.2.0")]),
                cand("2.0.0", Origin::Registry, &[]),
            ],
        );
        av.insert(
            "com.b".to_owned(),
            vec![
                cand("1.1.0", Origin::Registry, &[]),
                cand("1.3.0", Origin::Registry, &[]),
                cand("2.0.0", Origin::Registry, &[]),
            ],
        );
        let r = resolve(&map(&[("com.a", "^1.0.0")]), &av);
        assert!(r.conflicts.is_empty(), "{:?}", r.conflicts);
        assert_eq!(r.packages["com.a"].version.to_string(), "1.2.0");
        assert_eq!(r.packages["com.b"].version.to_string(), "1.3.0");
        assert!(r.packages["com.b"].indirect && !r.packages["com.a"].indirect);
        // The project asking for b ~1.1 too narrows it.
        let r = resolve(&map(&[("com.a", "1.0.0"), ("com.b", "~1.1.0")]), &av);
        assert_eq!(r.packages["com.b"].version.to_string(), "1.1.0");
    }

    #[test]
    fn conflicts_name_every_requirement() {
        let mut av = BTreeMap::new();
        av.insert(
            "com.a".to_owned(),
            vec![cand("1.0.0", Origin::Installed, &[("com.b", "^2.0.0")])],
        );
        av.insert(
            "com.b".to_owned(),
            vec![cand("1.5.0", Origin::Installed, &[])],
        );
        let r = resolve(&map(&[("com.a", "*"), ("com.b", "*")]), &av);
        assert_eq!(r.conflicts.len(), 1);
        let c = &r.conflicts[0];
        assert_eq!(c.name, "com.b");
        assert_eq!(c.requirements.len(), 2);
        let msg = c.message();
        assert!(
            msg.contains("com.a needs ^2.0.0") && msg.contains("project needs *"),
            "{msg}"
        );
        assert!(msg.contains("available: 1.5.0"), "{msg}");
        // Missing packages are conflicts too.
        let r = resolve(&map(&[("com.zzz", "^1.0.0")]), &av);
        assert!(r.conflicts[0].message().contains("no version is available"));
    }

    #[test]
    fn installed_packages_win_over_the_registry() {
        let mut av = BTreeMap::new();
        av.insert(
            "com.a".to_owned(),
            vec![
                cand("1.0.0", Origin::Installed, &[]),
                cand("1.9.0", Origin::Registry, &[]),
            ],
        );
        let r = resolve(&map(&[("com.a", "^1.0.0")]), &av);
        assert_eq!(r.packages["com.a"].version.to_string(), "1.0.0");
        assert_eq!(r.packages["com.a"].origin, Origin::Installed);
    }
}
