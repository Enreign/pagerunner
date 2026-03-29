import SwiftUI

struct RenameSheet: View {
    let title: String
    let prompt: String
    @Binding var isPresented: Bool
    let initialValue: String
    let onConfirm: (String) -> Void

    @State private var inputText: String = ""
    @FocusState private var isFocused: Bool

    var isValid: Bool {
        !inputText.trimmingCharacters(in: .whitespaces).isEmpty
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.headline)
            Text(prompt).font(.subheadline).foregroundStyle(.secondary)
            TextField("Name", text: $inputText)
                .textFieldStyle(.roundedBorder)
                .focused($isFocused)
                .onSubmit { if isValid { confirm() } }
            HStack {
                Spacer()
                Button("Cancel") { isPresented = false }
                Button("OK") { confirm() }
                    .buttonStyle(.borderedProminent)
                    .disabled(!isValid)
            }
        }
        .padding(20)
        .frame(minWidth: 280)
        .onAppear {
            inputText = initialValue
            isFocused = true
        }
    }

    private func confirm() {
        onConfirm(inputText.trimmingCharacters(in: .whitespaces))
        isPresented = false
    }
}
