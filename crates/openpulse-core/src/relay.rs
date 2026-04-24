use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayRouteError {
    EmptyRoute,
    LoopDetected { peer_id: String },
    TooManyHops { hops: usize, max_hops: usize },
    TrustPolicyRejected { peer_id: String },
    NoValidRoute,
}

#[derive(Debug, Clone, Default)]
pub struct RelayTrustPolicy {
    denied_relays: HashSet<String>,
}

impl RelayTrustPolicy {
    pub fn deny_relays<I, S>(denied: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            denied_relays: denied.into_iter().map(Into::into).collect(),
        }
    }

    pub fn allows(&self, relay_peer_id: &str) -> bool {
        !self.denied_relays.contains(relay_peer_id)
    }
}

pub fn validate_route_no_loops(route: &[String], max_hops: usize) -> Result<(), RelayRouteError> {
    if route.is_empty() {
        return Err(RelayRouteError::EmptyRoute);
    }

    let hops = route.len().saturating_sub(1);
    if hops > max_hops {
        return Err(RelayRouteError::TooManyHops { hops, max_hops });
    }

    let mut seen = HashSet::new();
    for peer in route {
        if !seen.insert(peer) {
            return Err(RelayRouteError::LoopDetected {
                peer_id: peer.clone(),
            });
        }
    }

    Ok(())
}

pub fn validate_route_with_policy(
    route: &[String],
    max_hops: usize,
    policy: &RelayTrustPolicy,
) -> Result<(), RelayRouteError> {
    validate_route_no_loops(route, max_hops)?;

    if route.len() <= 2 {
        return Ok(());
    }

    for relay in &route[1..route.len() - 1] {
        if !policy.allows(relay) {
            return Err(RelayRouteError::TrustPolicyRejected {
                peer_id: relay.clone(),
            });
        }
    }

    Ok(())
}

pub fn select_best_valid_route(
    candidates: &[Vec<String>],
    max_hops: usize,
    policy: &RelayTrustPolicy,
) -> Result<Vec<String>, RelayRouteError> {
    let mut best: Option<&Vec<String>> = None;

    for route in candidates {
        if validate_route_with_policy(route, max_hops, policy).is_ok() {
            match best {
                Some(current_best) if route.len() >= current_best.len() => {}
                _ => best = Some(route),
            }
        }
    }

    best.cloned().ok_or(RelayRouteError::NoValidRoute)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(peers: &[&str]) -> Vec<String> {
        peers.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn multi_hop_route_passes_when_within_hop_limit() {
        let route = route(&["src", "relay-a", "relay-b", "dst"]);
        assert!(validate_route_no_loops(&route, 3).is_ok());
    }

    #[test]
    fn route_fails_when_loop_is_detected() {
        let route = route(&["src", "relay-a", "relay-b", "relay-a", "dst"]);
        let err = validate_route_no_loops(&route, 5).expect_err("loop must be rejected");
        assert_eq!(
            err,
            RelayRouteError::LoopDetected {
                peer_id: "relay-a".to_string()
            }
        );
    }

    #[test]
    fn route_fails_when_hop_count_exceeds_limit() {
        let route = route(&["src", "relay-a", "relay-b", "dst"]);
        let err = validate_route_no_loops(&route, 2).expect_err("too many hops");
        assert_eq!(
            err,
            RelayRouteError::TooManyHops {
                hops: 3,
                max_hops: 2,
            }
        );
    }

    #[test]
    fn trust_policy_failure_rejects_route() {
        let policy = RelayTrustPolicy::deny_relays(["relay-b"]);
        let route = route(&["src", "relay-a", "relay-b", "dst"]);
        let err = validate_route_with_policy(&route, 3, &policy)
            .expect_err("untrusted relay must be rejected");
        assert_eq!(
            err,
            RelayRouteError::TrustPolicyRejected {
                peer_id: "relay-b".to_string()
            }
        );
    }

    #[test]
    fn selects_shortest_valid_route_and_skips_policy_failures() {
        let policy = RelayTrustPolicy::deny_relays(["relay-x"]);
        let candidates = vec![
            route(&["src", "relay-x", "dst"]),
            route(&["src", "relay-a", "relay-b", "dst"]),
            route(&["src", "relay-a", "dst"]),
        ];

        let selected = select_best_valid_route(&candidates, 4, &policy)
            .expect("a valid route should be selected");
        assert_eq!(selected, route(&["src", "relay-a", "dst"]));
    }

    #[test]
    fn no_valid_route_when_all_candidates_fail_policy_or_loops() {
        let policy = RelayTrustPolicy::deny_relays(["relay-a", "relay-b"]);
        let candidates = vec![
            route(&["src", "relay-a", "dst"]),
            route(&["src", "relay-b", "dst"]),
            route(&["src", "relay-c", "relay-c", "dst"]),
        ];

        let err = select_best_valid_route(&candidates, 4, &policy)
            .expect_err("all routes should be rejected");
        assert_eq!(err, RelayRouteError::NoValidRoute);
    }
}
