use std::hash::{Hash, Hasher};

pub fn compute_agent_id(host: &str, port: u16) -> i64 {
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        let ip_u32 = u32::from_be_bytes(ip.octets());
        (ip_u32 as i64) * 65536 + port as i64
    } else {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        host.hash(&mut hasher);
        port.hash(&mut hasher);
        let h = hasher.finish() as i64;
        if h == 0 { -1 } else { -h.abs() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_agent_id_ipv4() {
        let id = compute_agent_id("192.168.1.100", 9997);
        assert_eq!(id, 211827810379533);
    }

    #[test]
    fn test_compute_agent_id_hostname() {
        let id = compute_agent_id("agent-01.local", 9997);
        assert!(id < 0, "hostname should produce negative id");
    }

    #[test]
    fn test_compute_agent_id_uniqueness() {
        let id1 = compute_agent_id("10.0.0.1", 9997);
        let id2 = compute_agent_id("10.0.0.1", 9998);
        assert_ne!(id1, id2, "different ports should differ");
        let id3 = compute_agent_id("10.0.0.2", 9997);
        assert_ne!(id1, id3, "different IPs should differ");
    }
}
