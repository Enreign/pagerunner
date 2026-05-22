import SwiftUI
import PagerunnerKit

struct StatusBadge: View {
    let status: SessionStatus
    var showLabel: Bool = false

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(statusColor)
                .frame(width: 10, height: 10)

            if showLabel {
                Text(statusLabel)
                    .font(.caption)
                    .foregroundStyle(statusColor)
            }
        }
    }

    private var statusColor: Color {
        switch status {
        case .alive: .green
        case .crashed: .red
        case .reconnecting: .orange
        case .recovering: .yellow
        }
    }

    private var statusLabel: String {
        switch status {
        case .alive: "Alive"
        case .crashed: "Crashed"
        case .reconnecting: "Reconnecting"
        case .recovering: "Recovering"
        }
    }
}

#Preview("All Variants") {
    VStack(spacing: 16) {
        StatusBadge(status: .alive, showLabel: true)
        StatusBadge(status: .crashed, showLabel: true)
        StatusBadge(status: .reconnecting, showLabel: true)
        StatusBadge(status: .recovering, showLabel: true)

        Divider()

        HStack(spacing: 20) {
            StatusBadge(status: .alive)
            StatusBadge(status: .crashed)
            StatusBadge(status: .reconnecting)
        }
    }
    .padding()
}
