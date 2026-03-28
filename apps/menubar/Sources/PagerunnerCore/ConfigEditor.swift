import Foundation

// MARK: - URL extension

extension URL {
    static let pagerunnerConfig = URL(fileURLWithPath: NSHomeDirectory())
        .appendingPathComponent(".pagerunner/config.toml")
}

// MARK: - ConfigEditor errors

enum ConfigEditorError: Error, LocalizedError {
    case profileNotFound(String)
    case invalidProfileName(String)

    var errorDescription: String? {
        switch self {
        case .profileNotFound(let name):
            return "Profile '\(name)' not found in config"
        case .invalidProfileName(let reason):
            return "Invalid profile name: \(reason)"
        }
    }
}

// MARK: - ConfigEditor

struct ConfigEditor {

    // MARK: Public API

    static func renameProfile(
        name: String,
        newDisplayName: String,
        configURL: URL = .pagerunnerConfig
    ) throws {
        var blocks = try splitBlocks(readConfig(at: configURL))

        guard let idx = blocks.firstIndex(where: { blockMatchesName($0, name: name) }) else {
            throw ConfigEditorError.profileNotFound(name)
        }

        blocks[idx] = replaceDisplayName(in: blocks[idx], newDisplayName: newDisplayName)

        try writeConfig(blocks.joined(), to: configURL)
    }

    static func removeProfile(
        name: String,
        configURL: URL = .pagerunnerConfig
    ) throws {
        let blocks = try splitBlocks(readConfig(at: configURL))

        guard blocks.contains(where: { blockMatchesName($0, name: name) }) else {
            throw ConfigEditorError.profileNotFound(name)
        }

        let filtered = blocks.filter { !blockMatchesName($0, name: name) }
        try writeConfig(filtered.joined(), to: configURL)
    }

    static func addAttachedProfile(
        name: String,
        displayName: String,
        port: Int,
        configURL: URL = .pagerunnerConfig
    ) throws {
        guard !name.contains("\n") else {
            throw ConfigEditorError.invalidProfileName("name must not contain newlines")
        }
        guard !displayName.contains("\n") else {
            throw ConfigEditorError.invalidProfileName("display_name must not contain newlines")
        }
        let escapedName = name.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        let escapedDisplayName = displayName.replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        let existing = try readConfig(at: configURL)
        let newBlock = """


        [[profiles]]
        name = "\(escapedName)"
        display_name = "\(escapedDisplayName)"
        kind = "attached"
        debug_port = \(port)
        """
        try writeConfig(existing + newBlock, to: configURL)
    }

    // MARK: Internal helpers

    /// Splits the TOML content into blocks.
    /// The first block is the preamble (everything before the first [[profiles]]).
    /// Each [[profiles]] header starts a new block.
    static func splitBlocks(_ content: String) -> [String] {
        var blocks: [String] = []
        var current = ""

        let lines = content.components(separatedBy: "\n")
        for line in lines {
            if line.trimmingCharacters(in: .whitespaces) == "[[profiles]]" {
                blocks.append(current)
                current = line + "\n"
            } else {
                current += line + "\n"
            }
        }
        // Append the last accumulated block
        blocks.append(current)

        // Remove trailing newline artifact from the last block if needed,
        // but keep the blocks joinable as-is.
        return blocks
    }

    /// Returns true if the block contains a line `name = "<name>"` (after whitespace trim).
    /// A `display_name = ...` line that happens to contain `name = "..."` is NOT a match.
    static func blockMatchesName(_ block: String, name: String) -> Bool {
        let target = "name = \"\(name)\""
        return block.components(separatedBy: "\n").contains { line in
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            return trimmed == target && !trimmed.hasPrefix("display_name =")
        }
    }

    // MARK: Private helpers

    private static func replaceDisplayName(in block: String, newDisplayName: String) -> String {
        let lines = block.components(separatedBy: "\n")
        let updated = lines.map { line -> String in
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("display_name =") {
                // Preserve leading whitespace
                let trimmed = line.trimmingCharacters(in: .whitespaces)
                let leadingWhitespace = line.prefix(line.count - trimmed.count)
                let escaped = newDisplayName
                    .replacingOccurrences(of: "\\", with: "\\\\")
                    .replacingOccurrences(of: "\"", with: "\\\"")
                return "\(leadingWhitespace)display_name = \"\(escaped)\""
            }
            return line
        }
        return updated.joined(separator: "\n")
    }

    private static func readConfig(at url: URL) throws -> String {
        try String(contentsOf: url, encoding: .utf8)
    }

    private static func writeConfig(_ content: String, to url: URL) throws {
        let tmpURL = URL(fileURLWithPath: url.path + ".tmp")
        try content.write(to: tmpURL, atomically: true, encoding: .utf8)
        // Atomically replace: remove destination first if it exists, then move
        do {
            if FileManager.default.fileExists(atPath: url.path) {
                _ = try FileManager.default.replaceItemAt(url, withItemAt: tmpURL)
            } else {
                try FileManager.default.moveItem(at: tmpURL, to: url)
            }
        } catch {
            try? FileManager.default.removeItem(at: tmpURL)
            throw error
        }
    }
}
