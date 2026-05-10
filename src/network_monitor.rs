use std::net::TcpStream;
use std::time::Duration;

/// Check if network connectivity is available by attempting to connect to a reliable host
pub fn check_network_connectivity() -> bool {
    // Try to connect to Google DNS (8.8.8.8:53) with a short timeout
    // Port 53 is DNS, usually open even when other ports are blocked
    let timeout = Duration::from_secs(2);

    match TcpStream::connect_timeout(&"8.8.8.8:53".parse().unwrap(), timeout) {
        Ok(_) => true,
        Err(_) => {
            // Fallback to Cloudflare DNS
            match TcpStream::connect_timeout(&"1.1.1.1:53".parse().unwrap(), timeout) {
                Ok(_) => true,
                Err(_) => false,
            }
        }
    }
}

/// Check network connectivity with a custom timeout
pub fn check_network_connectivity_with_timeout(timeout_secs: u64) -> bool {
    let timeout = Duration::from_secs(timeout_secs);

    match TcpStream::connect_timeout(&"8.8.8.8:53".parse().unwrap(), timeout) {
        Ok(_) => true,
        Err(_) => {
            match TcpStream::connect_timeout(&"1.1.1.1:53".parse().unwrap(), timeout) {
                Ok(_) => true,
                Err(_) => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_connectivity() {
        let result = check_network_connectivity();
        println!("Network connectivity: {}", result);
    }
}
