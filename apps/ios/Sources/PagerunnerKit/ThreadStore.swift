import Foundation

/// Persists the user's threads as a single JSON file in a directory of choice.
/// Default directory is the app's Documents folder.
///
/// Reads/writes are synchronous. Writes are atomic. A corrupt or unreadable
/// file is treated as empty so the user can recover without manual file ops.
public struct ThreadStore: Sendable {
    private let directory: URL
    private let fileName: String
    private let encoder: JSONEncoder
    private let decoder: JSONDecoder

    public init(directory: URL? = nil, fileName: String = "threads.json") {
        self.directory = directory ?? Self.defaultDirectory()
        self.fileName = fileName

        let enc = JSONEncoder()
        enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        enc.dateEncodingStrategy = .iso8601
        self.encoder = enc

        let dec = JSONDecoder()
        dec.dateDecodingStrategy = .iso8601
        self.decoder = dec
    }

    private static func defaultDirectory() -> URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
    }

    private var fileURL: URL { directory.appendingPathComponent(fileName) }

    public func load() throws -> [ChatThread] {
        guard FileManager.default.fileExists(atPath: fileURL.path) else { return [] }
        let data = try Data(contentsOf: fileURL)
        do {
            return try decoder.decode([ChatThread].self, from: data)
        } catch {
            PgrLog.app.error("ThreadStore decode failed, treating as empty: \(error.localizedDescription, privacy: .public)")
            return []
        }
    }

    public func save(_ threads: [ChatThread]) throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let data = try encoder.encode(threads)
        try data.write(to: fileURL, options: .atomic)
    }
}
