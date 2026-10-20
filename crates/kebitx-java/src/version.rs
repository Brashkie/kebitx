//! Versions and version requirements, as the loaders read them.
//!
//! - Fabric and Quilt: semantic versions (`0.26.2+mc26.3`: build metadata
//!   after `+` is ignored, a pre-release after `-` sorts before the
//!   release) and predicates: `*`, `1.2.3` (exactly), `=`, `>`, `>=`, `<`,
//!   `<=`, `~1.2` (same minor), `^1.2` (same major), `1.2.x` (wildcard);
//!   terms separated by spaces must all hold; a list of predicates is
//!   any-of. A trailing `-` (`26.3-`) is the lowest pre-release, so
//!   `>=26.3-` accepts 26.3's own pre-releases.
//! - Forge and NeoForge: Maven ranges: `[1.21.1]`, `[1.0,2.0)`, `[1.0,)`,
//!   `(,1.0]`, several ranges any-of; a bare version is only a
//!   recommendation and accepts anything.
//!
//! What cannot be read exactly gives `None`: nothing is judged then.

use std::cmp::Ordering;

#[derive(Debug, Clone)]
pub struct Version {
    nums: Vec<u64>,
    /// `None`: a release; `Some(ids)`: a pre-release (`beta.1` -> `[beta, 1]`).
    pre: Option<Vec<String>>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let nums: Vec<u64> = core
            .split('.')
            .map(|n| n.parse().ok())
            .collect::<Option<_>>()?;
        if nums.is_empty() {
            return None;
        }
        Some(Self {
            nums,
            pre: pre.map(|p| {
                if p.is_empty() {
                    Vec::new()
                } else {
                    p.split('.').map(str::to_string).collect()
                }
            }),
        })
    }

    fn with_nums(nums: Vec<u64>) -> Self {
        // the lowest version with these numbers: before any pre-release
        Self {
            nums,
            pre: Some(Vec::new()),
        }
    }
}

fn cmp_pre(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(m), Ok(n)) => m.cmp(&n),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.nums.len().max(other.nums.len());
        for i in 0..n {
            let (a, b) = (
                self.nums.get(i).copied().unwrap_or(0),
                other.nums.get(i).copied().unwrap_or(0),
            );
            if a != b {
                return a.cmp(&b);
            }
        }
        match (&self.pre, &other.pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => cmp_pre(a, b),
        }
    }
}

impl PartialEq for Version {
    /// `26.3` and `26.3.0` are the same version.
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// One Fabric predicate term against a version.
fn fabric_term(term: &str, v: &Version) -> Option<bool> {
    if term == "*" {
        return Some(true);
    }
    let (op, rest) = [">=", "<=", ">", "<", "=", "~", "^"]
        .iter()
        .find_map(|op| term.strip_prefix(op).map(|r| (*op, r.trim())))
        .unwrap_or(("", term));
    // wildcards: `1.2.x`, `1.2.*`, `1.x`
    let parts: Vec<&str> = rest.split('+').next()?.split('.').collect();
    if let Some(w) = parts.iter().position(|p| matches!(*p, "x" | "X" | "*")) {
        if !matches!(op, "" | "=") {
            return None;
        }
        let prefix: Vec<u64> = parts[..w]
            .iter()
            .map(|p| p.parse().ok())
            .collect::<Option<_>>()?;
        return Some(
            prefix
                .iter()
                .enumerate()
                .all(|(i, n)| v.nums.get(i).copied().unwrap_or(0) == *n),
        );
    }
    let x = Version::parse(rest)?;
    Some(match op {
        "" | "=" => v.cmp(&x) == Ordering::Equal,
        ">=" => *v >= x,
        "<=" => *v <= x,
        ">" => *v > x,
        "<" => *v < x,
        "~" => {
            let mut upper = x.nums.clone();
            if upper.len() >= 2 {
                upper.truncate(2);
                upper[1] += 1;
            } else {
                upper[0] += 1;
            }
            *v >= x && *v < Version::with_nums(upper)
        }
        "^" => *v >= x && *v < Version::with_nums(vec![x.nums[0] + 1]),
        _ => return None,
    })
}

/// A Fabric predicate (`>=0.19.2 <0.20`) against a version string.
pub fn fabric_matches(predicate: &str, version: &str) -> Option<bool> {
    let p = predicate.trim();
    if p == "*" || p.is_empty() {
        return Some(true);
    }
    let v = Version::parse(version)?;
    let mut all = true;
    for term in p.split_whitespace() {
        all &= fabric_term(term, &v)?;
    }
    Some(all)
}

/// Any of several Fabric predicates.
pub fn fabric_any(predicates: &[String], version: &str) -> Option<bool> {
    let mut any = false;
    for p in predicates {
        any |= fabric_matches(p, version)?;
    }
    Some(any)
}

/// A Maven range (`[1.0,2.0)`, `[1.21.1]`, several any-of) against a version.
pub fn maven_matches(range: &str, version: &str) -> Option<bool> {
    let r = range.trim();
    if r.is_empty() || r == "*" {
        return Some(true);
    }
    if !r.starts_with(['[', '(']) {
        // a bare version is a recommendation: anything is accepted
        return Some(true);
    }
    let v = Version::parse(version)?;
    let mut rest = r;
    let mut any = false;
    while !rest.is_empty() {
        let open = rest.chars().next()?;
        let end = rest.find([']', ')'])?;
        let close = rest[end..].chars().next()?;
        let body = &rest[1..end];
        rest = rest[end + 1..].trim_start_matches(',').trim();
        if !matches!(open, '[' | '(') {
            return None;
        }
        let ok = match body.split_once(',') {
            None => {
                let x = Version::parse(body)?;
                v.cmp(&x) == Ordering::Equal
            }
            Some((lo, hi)) => {
                let lo_ok = match lo.trim() {
                    "" => true,
                    l => {
                        let l = Version::parse(l)?;
                        if open == '[' {
                            v >= l
                        } else {
                            v > l
                        }
                    }
                };
                let hi_ok = match hi.trim() {
                    "" => true,
                    h => {
                        let h = Version::parse(h)?;
                        if close == ']' {
                            v <= h
                        } else {
                            v < h
                        }
                    }
                };
                lo_ok && hi_ok
            }
        };
        any |= ok;
    }
    Some(any)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_as_semver() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("26.3") > v("26.2.9"));
        assert!(v("26.3") == v("26.3.0"));
        assert!(v("0.26.2+mc26.3") == v("0.26.2"));
        assert!(v("0.9.3-beta.1") < v("0.9.3"));
        assert!(v("0.9.3-beta.2") > v("0.9.3-beta.1"));
        assert!(v("0.9.3-beta.10") > v("0.9.3-beta.9"));
        assert!(v("26.3-") < v("26.3-rc.1"));
        assert!(Version::parse("mc26.3").is_none());
    }

    #[test]
    fn fabric_predicates() {
        let m = |p, v| fabric_matches(p, v).unwrap();
        assert!(m("*", "1.0"));
        assert!(m("~26.3", "26.3.2") && !m("~26.3", "26.4") && !m("~26.3", "26.2"));
        assert!(m("~26.3-", "26.3") && m(">=26.3-", "26.3"));
        assert!(m("26.3.x", "26.3.1") && !m("26.3.x", "26.4"));
        assert!(m(">=0.19.2", "0.19.3") && !m(">=0.19.2", "0.19.1"));
        assert!(m(">=1.0 <2.0", "1.5") && !m(">=1.0 <2.0", "2.0"));
        assert!(m("^1.2", "1.9") && !m("^1.2", "2.0"));
        assert!(m("<0.145.1", "0.145.0") && !m("<0.145.1", "0.163.0+26.3"));
        assert!(m("1.2.3", "1.2.3") && !m("1.2.3", "1.2.4"));
        assert_eq!(fabric_matches(">=1.0", "not-a-version"), None);
        assert_eq!(
            fabric_any(&["~26.2".into(), "~26.3".into()], "26.3"),
            Some(true)
        );
    }

    #[test]
    fn maven_ranges() {
        let m = |r, v| maven_matches(r, v).unwrap();
        assert!(m("[1.21.1]", "1.21.1") && !m("[1.21.1]", "1.21.2"));
        assert!(m("[1.0,2.0)", "1.9") && !m("[1.0,2.0)", "2.0"));
        assert!(m("[21.1.219,)", "21.1.300") && !m("[21.1.219,)", "21.1.1"));
        assert!(m("(,1.0]", "1.0") && !m("(,1.0]", "1.1"));
        assert!(m("[1,2),[3,4)", "3.5") && !m("[1,2),[3,4)", "2.5"));
        assert!(m("1.0", "9.9"), "a bare version only recommends");
    }
}
