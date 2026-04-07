import SwiftUI

struct AgentApprovalCard: View {
    let action: String
    let description: String
    let onApprove: () -> Void
    let onDeny: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 4) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .font(.system(size: 12))
                Text("Approval Required")
                    .font(.system(size: 12, weight: .semibold))
            }
            .foregroundColor(Color(red: 0.961, green: 0.620, blue: 0.043))

            Text(description)
                .font(.system(size: 12))
                .foregroundColor(Color(red: 0.114, green: 0.114, blue: 0.122))
                .lineLimit(3)

            HStack(spacing: 12) {
                Button(action: onApprove) {
                    Text("Approve")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(.white)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 5)
                        .background(Color(red: 0.133, green: 0.773, blue: 0.369))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)

                Button(action: onDeny) {
                    Text("Deny")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundColor(Color(red: 0.937, green: 0.267, blue: 0.267))
                        .padding(.horizontal, 16)
                        .padding(.vertical, 5)
                        .background(Color(red: 0.937, green: 0.267, blue: 0.267).opacity(0.1))
                        .cornerRadius(5)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(12)
        .background(Color(red: 0.961, green: 0.620, blue: 0.043).opacity(0.08))
        .cornerRadius(8)
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color(red: 0.961, green: 0.620, blue: 0.043).opacity(0.3), lineWidth: 1)
        )
        .padding(.horizontal, 12)
    }
}
