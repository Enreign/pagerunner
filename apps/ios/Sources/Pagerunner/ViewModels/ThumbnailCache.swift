import Foundation
import UIKit
import PagerunnerKit

@Observable @MainActor
final class ThumbnailCache {
    private(set) var images: [String: UIImage] = [:]
    private var inflight: Set<String> = []

    private func key(_ ctx: PinnedContext) -> String {
        "\(ctx.sessionId)-\(ctx.targetId ?? "first")"
    }

    func image(for ctx: PinnedContext) -> UIImage? {
        images[key(ctx)]
    }

    /// Fetch a thumbnail for the given context. No-op if already cached or
    /// in-flight. Uses the first tab if `targetId` is nil.
    func fetchIfNeeded(_ ctx: PinnedContext, client: APIClient) {
        let k = key(ctx)
        guard images[k] == nil, !inflight.contains(k) else { return }
        inflight.insert(k)
        Task {
            defer { inflight.remove(k) }
            let targetId: String?
            if let tid = ctx.targetId {
                targetId = tid
            } else {
                targetId = (try? await client.listTabs(sessionId: ctx.sessionId))?.first?.targetId
            }
            guard let tid = targetId else { return }
            do {
                let base64 = try await client.screenshot(sessionId: ctx.sessionId, targetId: tid)
                guard let data = Data(base64Encoded: base64),
                      let img = UIImage(data: data) else { return }
                images[k] = img
            } catch {
                // Silently skip on failure; chip falls back to placeholder.
            }
        }
    }

    func invalidate(_ ctx: PinnedContext) {
        images.removeValue(forKey: key(ctx))
    }
}
