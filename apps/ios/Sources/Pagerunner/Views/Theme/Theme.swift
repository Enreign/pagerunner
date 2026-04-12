import SwiftUI

enum Theme {
    enum Radius {
        static let card: CGFloat = 16
        static let pill: CGFloat = 999
        static let chip: CGFloat = 8
    }

    enum Spacing {
        static let tight: CGFloat = 8
        static let regular: CGFloat = 12
        static let loose: CGFloat = 20
        static let section: CGFloat = 24
    }
}

extension Font {
    static let mono = Font.system(.body, design: .monospaced)
    static let monoCaption = Font.system(.caption, design: .monospaced)
    static let monoFootnote = Font.system(.footnote, design: .monospaced)
    static let statNumber = Font.system(size: 34, weight: .semibold, design: .rounded)
    static let statLabel = Font.system(size: 11, weight: .medium).smallCaps()
}

extension ShapeStyle where Self == Color {
    static var operatorBackground: Color { Color(.systemGroupedBackground) }
    static var operatorCard: Color { Color(.secondarySystemGroupedBackground) }
    static var operatorSubtle: Color { Color(.tertiarySystemGroupedBackground) }
    static var accent: Color { Color("AccentColor") }
}
