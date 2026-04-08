import SwiftUI

/// Compact voice status indicator: colored dot + label.
struct VoiceStatusBadge: View {
    let status: AppState.VoiceStatus

    var body: some View {
        HStack(spacing: 4) {
            Circle()
                .fill(dotColor)
                .frame(width: 6, height: 6)
            Text(label)
                .font(.system(size: 11))
                .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
        }
    }

    private var dotColor: Color {
        switch status {
        case .idle:
            return Color(red: 0.533, green: 0.533, blue: 0.533)
        case .starting:
            return Color(red: 0.961, green: 0.620, blue: 0.043)
        case .listening:
            return Color(red: 0.133, green: 0.773, blue: 0.369)
        case .processing:
            return Color(red: 0.961, green: 0.620, blue: 0.043)
        case .speaking:
            return Color(red: 0, green: 0.478, blue: 1)
        }
    }

    private var label: String {
        switch status {
        case .idle:
            return "Voice idle"
        case .starting:
            return "Starting..."
        case .listening:
            return "Listening..."
        case .processing:
            return "Processing..."
        case .speaking:
            return "Speaking..."
        }
    }
}
