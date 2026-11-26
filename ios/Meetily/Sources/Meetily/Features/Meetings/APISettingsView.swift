import SwiftUI

struct APISettingsView: View {
    let onSave: () -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var baseURL = MeetilyAPIClient.savedBaseURL().absoluteString
    @State private var validationMessage: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("http://127.0.0.1:8000", text: $baseURL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                } header: {
                    Text("Meetily backend URL")
                } footer: {
                    Text("Use your computer’s LAN IP when testing on a physical iPhone.")
                }

                if let validationMessage {
                    Section {
                        Text(validationMessage)
                            .foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { save() }
                }
            }
        }
    }

    private func save() {
        guard let url = URL(string: baseURL.trimmingCharacters(in: .whitespacesAndNewlines)),
              url.scheme != nil,
              url.host != nil else {
            validationMessage = "Enter a valid URL, for example http://192.168.1.20:8000."
            return
        }

        MeetilyAPIClient.saveBaseURL(url)
        onSave()
        dismiss()
    }
}
