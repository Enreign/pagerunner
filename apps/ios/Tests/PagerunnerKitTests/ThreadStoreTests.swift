import Testing
import Foundation
@testable import PagerunnerKit

@Suite("ThreadStore")
struct ThreadStoreTests {

    private func freshStore() throws -> (ThreadStore, URL) {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("threadstore-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return (ThreadStore(directory: dir), dir)
    }

    @Test func loadsEmptyWhenFileMissing() throws {
        let (store, _) = try freshStore()
        #expect(try store.load().isEmpty)
    }

    @Test func saveThenLoadRoundtrip() throws {
        let (store, _) = try freshStore()
        let thread = ChatThread(
            title: "test",
            pinnedContext: PinnedContext(sessionId: "s-1"),
            records: [.user(id: UUID(), text: "hi", at: Date(timeIntervalSince1970: 1))]
        )
        try store.save([thread])
        let loaded = try store.load()
        #expect(loaded.count == 1)
        #expect(loaded[0].id == thread.id)
        #expect(loaded[0].title == "test")
        #expect(loaded[0].pinnedContext?.sessionId == "s-1")
        #expect(loaded[0].records.count == 1)
    }

    @Test func saveOverwrites() throws {
        let (store, _) = try freshStore()
        try store.save([ChatThread(title: "first")])
        try store.save([ChatThread(title: "second")])
        let loaded = try store.load()
        #expect(loaded.count == 1)
        #expect(loaded[0].title == "second")
    }

    @Test func corruptFileTreatedAsEmpty() throws {
        let (store, dir) = try freshStore()
        try Data("not json".utf8).write(to: dir.appendingPathComponent("threads.json"))
        // Corrupt file should not crash; treated as empty so the user can recover.
        #expect(try store.load().isEmpty)
    }
}
