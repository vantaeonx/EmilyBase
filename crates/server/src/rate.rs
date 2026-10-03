use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);
const MAX_PEERS: usize = 4096;
const REQUESTS: u32 = 120;
#[derive(Default)]
pub(crate) struct Rate {
    peers: BTreeMap<IpAddr, (Instant, u32)>,
}
impl Rate {
    pub(crate) fn admits(&mut self, peer: IpAddr, now: Instant) -> bool {
        if let Some((start, count)) = self.peers.get_mut(&peer) {
            if now.duration_since(*start) >= WINDOW {
                *start = now;
                *count = 0;
            }
            if *count >= REQUESTS {
                return false;
            }
            *count += 1;
            return true;
        }
        if self.peers.len() >= MAX_PEERS {
            self.peers
                .retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
            if self.peers.len() >= MAX_PEERS {
                return false;
            }
        }
        self.peers.insert(peer, (now, 1));
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_request_boundary_resets_only_after_monotonic_window() {
        let mut rate = Rate::default();
        let peer = IpAddr::from([127, 0, 0, 1]);
        let now = Instant::now();
        for _ in 0..REQUESTS {
            assert!(rate.admits(peer, now));
        }
        assert!(!rate.admits(peer, now));
        assert!(!rate.admits(peer, now + WINDOW - Duration::from_nanos(1)));
        assert!(rate.admits(peer, now + WINDOW));
    }
    #[test]
    fn full_peer_map_is_bounded_and_expired_entries_are_reclaimed() {
        let mut rate = Rate::default();
        let now = Instant::now();
        for i in 0..MAX_PEERS {
            assert!(rate.admits(IpAddr::V4(std::net::Ipv4Addr::from(i as u32)), now));
        }
        let extra = IpAddr::from([255, 255, 255, 255]);
        assert!(!rate.admits(extra, now));
        assert_eq!(rate.peers.len(), MAX_PEERS);
        assert!(rate.admits(extra, now + WINDOW));
        assert_eq!(rate.peers.len(), 1);
    }
}
