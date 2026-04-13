import SwiftUI

/// Unified input bar for both idle and running agent states.
/// Supports text input, voice toggle (tap or push-to-talk), and send/stop actions.
struct AgentInputBar: View {
    @Binding var text: String
    let placeholder: String
    let voiceActive: Bool
    let voiceStatus: AppState.VoiceStatus
    let voiceMode: AppState.VoiceMode
    let isRunning: Bool
    let onSend: () -> Void
    let onStop: () -> Void
    let onMicTap: () -> Void
    let onMicHoldStart: () -> Void
    let onMicHoldEnd: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            // Text field or voice listening indicator
            if voiceActive && voiceStatus == .listening {
                HStack(spacing: 6) {
                    Circle()
                        .fill(Color(red: 0.133, green: 0.773, blue: 0.369))
                        .frame(width: 8, height: 8)
                    Text("Listening...")
                        .font(.system(size: 13))
                        .foregroundColor(Color(red: 0.533, green: 0.533, blue: 0.533))
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(Color.white.opacity(0.92))
                .clipShape(.rect(cornerRadius: 12))
                .overlay(
                    RoundedRectangle(cornerRadius: 12)
                        .stroke(Color(red: 0.133, green: 0.773, blue: 0.369).opacity(0.3), lineWidth: 1)
                )
            } else {
                TextField(placeholder, text: $text, axis: .vertical)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .padding(.horizontal, 12)
                    .padding(.vertical, 10)
                    .lineLimit(1...3)
                    .background(Color.white.opacity(0.92))
                    .clipShape(.rect(cornerRadius: 12))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12)
                            .stroke(Color.primary.opacity(0.15), lineWidth: 0.5)
                    )
                    .onSubmit {
                        if !text.isEmpty { onSend() }
                    }
            }

            // Mic button
            micButton

            // Send or Stop button
            if isRunning {
                Button(action: onStop) {
                    Image(systemName: "stop.fill")
                        .font(.system(size: 12))
                        .foregroundColor(.white)
                        .frame(width: 34, height: 34)
                        .background(Color(red: 0.937, green: 0.267, blue: 0.267).opacity(0.8))
                        .clipShape(.rect(cornerRadius: 12))
                }
                .buttonStyle(.plain)
            } else {
                Button(action: onSend) {
                    Image(systemName: "arrow.up")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundColor(.white)
                        .frame(width: 34, height: 34)
                        .background(text.isEmpty
                            ? Color.gray.opacity(0.3)
                            : Color(red: 0, green: 0.478, blue: 1))
                        .clipShape(.rect(cornerRadius: 12))
                }
                .buttonStyle(.plain)
                .disabled(text.isEmpty)
            }
        }
    }

    @ViewBuilder
    private var micButton: some View {
        let isActive = voiceActive
        let color: Color = isActive
            ? Color(red: 0.937, green: 0.267, blue: 0.267)
            : Color(red: 0.533, green: 0.533, blue: 0.533)

        if voiceMode == .pushToTalk && isActive {
            Image(systemName: "mic.fill")
                .font(.system(size: 14))
                .foregroundColor(.white)
                .frame(width: 34, height: 34)
                .background(color)
                .clipShape(.rect(cornerRadius: 12))
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { _ in onMicHoldStart() }
                        .onEnded { _ in onMicHoldEnd() }
                )
        } else {
            Button(action: onMicTap) {
                Image(systemName: isActive ? "mic.fill" : "mic")
                    .font(.system(size: 14))
                    .foregroundColor(isActive ? .white : color)
                    .frame(width: 34, height: 34)
                    .background(isActive ? color : Color.white.opacity(0.92))
                    .clipShape(.rect(cornerRadius: 12))
                    .overlay(
                        RoundedRectangle(cornerRadius: 12)
                            .stroke(isActive ? Color.clear : Color.primary.opacity(0.12), lineWidth: 0.5)
                    )
            }
            .buttonStyle(.plain)
        }
    }
}
