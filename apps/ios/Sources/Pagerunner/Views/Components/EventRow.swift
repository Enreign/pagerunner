import SwiftUI
import PagerunnerKit

struct EventRow: View {
    let event: AgentEventDetail
    var onApprove: (() -> Void)?
    var onDeny: (() -> Void)?

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            eventIcon
                .frame(width: 28, height: 28)

            VStack(alignment: .leading, spacing: 6) {
                Text(eventTitle)
                    .font(.subheadline.bold())
                    .foregroundStyle(eventTitleColor)

                if let description = eventDescription {
                    Text(description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(4)
                }

                if isApprovalRequired {
                    approvalButtons
                }
            }

            Spacer(minLength: 0)
        }
        .padding(12)
        .background(eventBackground)
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }

    // MARK: - Icon

    @ViewBuilder
    private var eventIcon: some View {
        switch event {
        case .thinking:
            Image(systemName: "brain")
                .foregroundStyle(.gray)
        case .toolCall:
            Image(systemName: "hammer.fill")
                .foregroundStyle(.blue)
        case .toolResult(_, _, let isError):
            Image(systemName: isError ? "xmark.circle.fill" : "checkmark.circle.fill")
                .foregroundStyle(isError ? .red : .green)
        case .progress:
            Image(systemName: "arrow.right.circle.fill")
                .foregroundStyle(.teal)
        case .approvalRequired:
            Image(systemName: "exclamationmark.shield.fill")
                .foregroundStyle(.orange)
        case .approvalResponse(_, let approved):
            Image(systemName: approved ? "hand.thumbsup.fill" : "hand.thumbsdown.fill")
                .foregroundStyle(approved ? .green : .red)
        case .done:
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(.green)
        case .error:
            Image(systemName: "xmark.circle.fill")
                .foregroundStyle(.red)
        case .interrupted:
            Image(systemName: "stop.circle.fill")
                .foregroundStyle(.orange)
        case .budgetExceeded:
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.yellow)
        case .scopeDigest:
            Image(systemName: "doc.text.magnifyingglass")
                .foregroundStyle(.secondary)
        case .turnSummary:
            Image(systemName: "list.bullet.clipboard")
                .foregroundStyle(.secondary)
        case .unknown:
            Image(systemName: "questionmark.circle.fill")
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Content

    private var eventTitle: String {
        switch event {
        case .thinking: "Thinking"
        case .toolCall(let name, _): "Tool Call: \(name)"
        case .toolResult(let name, _, let isError): isError ? "Failed: \(name)" : "Success: \(name)"
        case .progress: "Progress"
        case .approvalRequired(_, let action, _): "Approval: \(action)"
        case .approvalResponse(_, let approved): approved ? "Approved" : "Denied"
        case .done: "Complete"
        case .error(let message, _): "Error: \(message.prefix(40))"
        case .interrupted: "Interrupted"
        case .budgetExceeded: "Budget Exceeded"
        case .scopeDigest: "Scope Digest"
        case .turnSummary: "Turn Summary"
        case .unknown(let type): "Unknown: \(type)"
        }
    }

    private var eventTitleColor: Color {
        switch event {
        case .error: .red
        case .approvalRequired: .orange
        case .done: .green
        case .budgetExceeded: .yellow
        case .interrupted: .orange
        default: .primary
        }
    }

    private var eventDescription: String? {
        switch event {
        case .thinking(let text): text
        case .toolCall(_, let args): args.stringValue ?? "..."
        case .toolResult(_, let result, _): String(result.prefix(200))
        case .progress(let message): message
        case .approvalRequired(_, _, let description): description
        case .approvalResponse: nil
        case .done(let summary): summary
        case .error(let message, _): message
        case .interrupted: "Agent was interrupted by user."
        case .budgetExceeded(let reason): reason
        case .scopeDigest(_, _, let digest): digest
        case .turnSummary(let summary, _): summary
        case .unknown: nil
        }
    }

    private var eventBackground: Color {
        switch event {
        case .approvalRequired: Color.orange.opacity(0.08)
        case .error: Color.red.opacity(0.08)
        case .done: Color.green.opacity(0.08)
        default: Color(.tertiarySystemFill)
        }
    }

    private var isApprovalRequired: Bool {
        if case .approvalRequired = event { return true }
        return false
    }

    // MARK: - Approval Buttons

    private var approvalButtons: some View {
        HStack(spacing: 12) {
            Button {
                onApprove?()
            } label: {
                Label("Approve", systemImage: "checkmark")
                    .font(.caption.bold())
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(.green)
                    .foregroundStyle(.white)
                    .clipShape(Capsule())
            }

            Button {
                onDeny?()
            } label: {
                Label("Deny", systemImage: "xmark")
                    .font(.caption.bold())
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(.red)
                    .foregroundStyle(.white)
                    .clipShape(Capsule())
            }
        }
        .padding(.top, 4)
    }
}

#Preview("All Event Types") {
    ScrollView {
        VStack(spacing: 8) {
            EventRow(event: .thinking(text: "Analyzing the page structure..."))
            EventRow(event: .toolCall(name: "click", args: .string("selector: #login-btn")))
            EventRow(event: .toolResult(name: "click", result: "Click succeeded", isError: false))
            EventRow(event: .toolResult(name: "click", result: "Element not found: #missing-btn", isError: true))
            EventRow(event: .progress(message: "Step 3 of 5"))
            EventRow(
                event: .approvalRequired(
                    runId: "run-1",
                    action: "navigate",
                    description: "Agent wants to navigate to https://example.com/admin"
                ),
                onApprove: {},
                onDeny: {}
            )
            EventRow(event: .done(summary: "Task completed successfully"))
            EventRow(event: .error(message: "Connection timeout after 30s", recoverable: false))
            EventRow(event: .interrupted)
            EventRow(event: .budgetExceeded(reason: "Max steps (50) reached"))
        }
        .padding()
    }
}
