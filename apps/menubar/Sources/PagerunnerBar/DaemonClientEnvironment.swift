import SwiftUI
import PagerunnerCore

// MARK: - Environment key for DaemonClient

struct DaemonClientKey: EnvironmentKey {
    static let defaultValue = DaemonClient()
}

extension EnvironmentValues {
    var daemonClient: DaemonClient {
        get { self[DaemonClientKey.self] }
        set { self[DaemonClientKey.self] = newValue }
    }
}
