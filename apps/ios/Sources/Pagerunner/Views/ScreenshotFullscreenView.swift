import SwiftUI

struct ScreenshotFullscreenView: View {
    let image: UIImage
    let caption: ChatItem.Caption?

    @Environment(\.dismiss) private var dismiss
    @State private var scale: CGFloat = 1
    @GestureState private var pinch: CGFloat = 1

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            Image(uiImage: image)
                .resizable()
                .scaledToFit()
                .scaleEffect(scale * pinch)
                .gesture(
                    MagnifyGesture()
                        .updating($pinch) { value, state, _ in state = value.magnification }
                        .onEnded { value in
                            scale = max(1, min(scale * value.magnification, 5))
                        }
                )
                .onTapGesture(count: 2) {
                    withAnimation(.spring()) { scale = scale > 1 ? 1 : 2 }
                }

            VStack {
                HStack {
                    Spacer()
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.title)
                            .foregroundStyle(.white, .black.opacity(0.5))
                    }
                    .padding()
                }
                Spacer()
                if let caption {
                    HStack(spacing: 8) {
                        Image(systemName: "globe")
                            .foregroundStyle(.white.opacity(0.6))
                        VStack(alignment: .leading, spacing: 1) {
                            Text(caption.title.isEmpty ? caption.host : caption.title)
                                .font(.footnote.weight(.medium))
                                .foregroundStyle(.white)
                                .lineLimit(1)
                            if !caption.title.isEmpty {
                                Text(caption.host)
                                    .font(.caption2)
                                    .foregroundStyle(.white.opacity(0.7))
                                    .lineLimit(1)
                            }
                        }
                        Spacer(minLength: 0)
                    }
                    .padding()
                    .background(.black.opacity(0.5))
                }
            }
        }
    }
}
