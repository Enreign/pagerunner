import SwiftUI
import PagerunnerKit

struct NetworkLogRow: View {
    let entry: NetworkLogEntry

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            methodBadge

            VStack(alignment: .leading, spacing: 4) {
                Text(displayURL)
                    .font(.caption)
                    .monospaced()
                    .lineLimit(2)
                    .truncationMode(.middle)

                HStack(spacing: 12) {
                    statusCodeLabel

                    Label(formatDuration(entry.durationMs), systemImage: "clock")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
    }

    // MARK: - Method Badge

    private var methodBadge: some View {
        Text(entry.method)
            .font(.caption2.bold())
            .monospaced()
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(methodColor.opacity(0.15))
            .foregroundStyle(methodColor)
            .clipShape(RoundedRectangle(cornerRadius: 4))
            .frame(width: 50)
    }

    private var methodColor: Color {
        switch entry.method.uppercased() {
        case "GET": .blue
        case "POST": .green
        case "PUT": .orange
        case "PATCH": .yellow
        case "DELETE": .red
        default: .secondary
        }
    }

    // MARK: - Status Code

    private var statusCodeLabel: some View {
        Text("\(entry.status)")
            .font(.caption2.bold())
            .monospaced()
            .foregroundStyle(statusCodeColor(Int(entry.status)))
    }

    private func statusCodeColor(_ code: Int) -> Color {
        switch code {
        case 200..<300: .green
        case 300..<400: .blue
        case 400..<500: .orange
        case 500..<600: .red
        default: .secondary
        }
    }

    // MARK: - Formatting

    private var displayURL: String {
        if let url = URL(string: entry.url) {
            return url.path + (url.query.map { "?\($0)" } ?? "")
        }
        return entry.url
    }

    private func formatDuration(_ ms: UInt64) -> String {
        if ms < 1000 {
            return "\(ms)ms"
        } else {
            return String(format: "%.1fs", Double(ms) / 1000)
        }
    }
}

#Preview {
    List {
        NetworkLogRow(entry: NetworkLogEntry(
            requestId: "r1",
            url: "https://api.example.com/v1/users?page=1",
            method: "GET",
            status: 200,
            durationMs: 142,
            timestampMs: 1700000000000,
            requestHeaders: nil,
            requestBody: nil,
            responseBody: nil,
            responseTruncated: nil,
            tabId: "t1"
        ))

        NetworkLogRow(entry: NetworkLogEntry(
            requestId: "r2",
            url: "https://api.example.com/v1/sessions",
            method: "POST",
            status: 201,
            durationMs: 320,
            timestampMs: 1700000001000,
            requestHeaders: nil,
            requestBody: nil,
            responseBody: nil,
            responseTruncated: nil,
            tabId: "t1"
        ))

        NetworkLogRow(entry: NetworkLogEntry(
            requestId: "r3",
            url: "https://api.example.com/v1/users/123",
            method: "DELETE",
            status: 403,
            durationMs: 89,
            timestampMs: 1700000002000,
            requestHeaders: nil,
            requestBody: nil,
            responseBody: nil,
            responseTruncated: nil,
            tabId: "t1"
        ))

        NetworkLogRow(entry: NetworkLogEntry(
            requestId: "r4",
            url: "https://api.example.com/v1/config",
            method: "PUT",
            status: 500,
            durationMs: 5200,
            timestampMs: 1700000003000,
            requestHeaders: nil,
            requestBody: nil,
            responseBody: nil,
            responseTruncated: nil,
            tabId: "t1"
        ))
    }
    .listStyle(.plain)
}
