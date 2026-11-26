import Foundation
import Combine

@MainActor
final class MeetingsViewModel: ObservableObject {
    @Published private(set) var meetings: [Meeting] = []
    @Published private(set) var isLoading = false
    @Published var errorMessage: String?

    let apiClient: MeetilyAPIClient

    init(apiClient: MeetilyAPIClient = MeetilyAPIClient()) {
        self.apiClient = apiClient
    }

    func load() async {
        guard !isLoading else { return }

        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            meetings = try await apiClient.fetchMeetings()
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
