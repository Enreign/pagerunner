import SwiftUI

/// Styled result card shown after the agent completes a run.
struct AgentResultCard: View {
    let summary: String
    let steps: Int
    let tokens: Int
    let voiceActive: Bool
    let onReplay: () -> Void
    let onCopy: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            // Result header
            HStack(spacing: 6) {
                Image(systemName: "doc.text")
                    .font(.system(size: 12))
                    .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                Text("Result")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
            }

            // Summary text
            Text(summary)
                .font(.system(size: 12))
                .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))
                .textSelection(.enabled)

            Divider()

            // Stats + actions
            HStack(spacing: 0) {
                Text("\u{2713} \(steps) steps \u{00B7} \(formatTokens(tokens))")
                    .font(.system(size: 11))
                    .foregroundColor(Color(red: 0.133, green: 0.773, blue: 0.369))

                Spacer()

                if voiceActive {
                    Button(action: onReplay) {
                        HStack(spacing: 3) {
                            Image(systemName: "speaker.wave.2")
                                .font(.system(size: 10))
                            Text("Replay")
                                .font(.system(size: 11))
                        }
                    }
                    .buttonStyle(.plain)
                    .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
                    .padding(.trailing, 10)
                }

                Button(action: onCopy) {
                    HStack(spacing: 3) {
                        Image(systemName: "doc.on.doc")
                            .font(.system(size: 10))
                        Text("Copy")
                            .font(.system(size: 11))
                    }
                }
                .buttonStyle(.plain)
                .foregroundColor(Color(red: 0, green: 0.478, blue: 1))
            }
        }
        .padding(12)
        .background(Color(red: 0, green: 0.478, blue: 1).opacity(0.04))
        .cornerRadius(10)
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .stroke(Color(red: 0, green: 0.478, blue: 1).opacity(0.15), lineWidth: 0.5)
        )
        .padding(.horizontal, 12)
    }

    private func formatTokens(_ t: Int) -> String {
        t >= 1000 ? "\(t / 1000)K tokens" : "\(t) tokens"
    }
}
