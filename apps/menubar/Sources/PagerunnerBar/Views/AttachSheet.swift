import SwiftUI
import PagerunnerCore

/// Sheet shown when attaching a discovered Chrome instance.
/// Lets the user either create a new profile or merge the port into an existing one.
@MainActor
struct AttachSheet: View {
    let instance: DiscoveredInstance
    let existingProfiles: [Profile]
    @Binding var isPresented: Bool
    let onAttachNew: (String) -> Void        // displayName
    let onMergeInto: (Profile) -> Void       // existing profile

    @State private var mode: Mode = .new
    @State private var newName: String = ""
    @FocusState private var focused: Bool

    enum Mode { case new, existing }

    private var defaultName: String {
        instance.isVM ? "Chrome :\(instance.port) (VM)" : "Chrome :\(instance.port)"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Attach Chrome :\(instance.port)")
                .font(.system(size: 13, weight: .semibold))

            Picker("", selection: $mode) {
                Text("New profile").tag(Mode.new)
                Text("Existing profile").tag(Mode.existing)
            }
            .pickerStyle(.segmented)
            .labelsHidden()

            if mode == .new {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Profile name")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(.secondary)
                    TextField("Display name", text: $newName)
                        .textFieldStyle(.plain)
                        .font(.system(size: 12))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 5)
                        .background(Color.primary.opacity(0.06))
                        .cornerRadius(5)
                        .overlay(RoundedRectangle(cornerRadius: 5)
                            .stroke(Color.primary.opacity(0.15), lineWidth: 0.5))
                        .focused($focused)
                        .onSubmit { confirm() }
                }
                .onAppear {
                    newName = defaultName
                    focused = true
                }
            } else {
                if existingProfiles.isEmpty {
                    Text("No existing profiles")
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                } else {
                    VStack(spacing: 0) {
                        ForEach(Array(existingProfiles.enumerated()), id: \.element.id) { index, profile in
                            ProfilePickerRow(
                                profile: profile,
                                index: index,
                                onSelect: {
                                    isPresented = false
                                    onMergeInto(profile)
                                }
                            )
                        }
                    }
                    .background(Color.primary.opacity(0.04))
                    .cornerRadius(8)
                }
            }

            if mode == .new {
                HStack {
                    Button("Cancel") { isPresented = false }
                        .buttonStyle(.plain)
                        .foregroundColor(.secondary)
                    Spacer()
                    Button("Attach") { confirm() }
                        .buttonStyle(.borderedProminent)
                        .disabled(newName.trimmingCharacters(in: .whitespaces).isEmpty)
                        .keyboardShortcut(.return)
                }
            }
        }
        .padding(20)
        .frame(width: 280)
    }

    private func confirm() {
        let name = newName.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        isPresented = false
        onAttachNew(name)
    }
}

private struct ProfilePickerRow: View {
    let profile: Profile
    let index: Int
    let onSelect: () -> Void
    @State private var isHovered = false

    var body: some View {
        Button(action: onSelect) {
            HStack(spacing: 8) {
                ProfileIcon(profile: profile, index: index, size: 20)
                Text(profile.displayName)
                    .font(.system(size: 12))
                    .foregroundColor(Color(red: 0.133, green: 0.133, blue: 0.133))
                    .lineLimit(1)
                Spacer()
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(isHovered ? Color(red: 0, green: 0.478, blue: 1).opacity(0.08) : Color.clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovered = $0 }
    }
}
