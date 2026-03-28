// apps/menubar/Sources/PagerunnerCore/URLHelpers.swift
import Foundation

/// Extracts the origin (scheme + host + optional port) from a URL string.
/// Returns nil for non-http(s) URLs or malformed strings.
/// Examples:
///   "https://linear.app/foo/bar" → "https://linear.app"
///   "http://localhost:3000/x"    → "http://localhost:3000"
///   "chrome://newtab"            → nil
public func originFrom(url urlString: String) -> String? {
    guard let url = URL(string: urlString),
          let scheme = url.scheme,
          scheme == "https" || scheme == "http",
          let host = url.host else { return nil }
    if let port = url.port {
        return "\(scheme)://\(host):\(port)"
    }
    return "\(scheme)://\(host)"
}
