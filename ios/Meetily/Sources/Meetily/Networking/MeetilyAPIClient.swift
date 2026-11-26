import Foundation

enum MeetilyAPIError: LocalizedError {
    case invalidResponse
    case server(statusCode: Int)

    var errorDescription: String? {
        switch self {
        case .invalidResponse:
            return "Meetily returned an invalid response."
        case let .server(statusCode):
            return "Meetily returned HTTP \(statusCode)."
        }
    }
}

struct MeetilyAPIClient: Sendable {
    let baseURL: URL

    init(baseURL: URL = MeetilyAPIClient.savedBaseURL()) {
        self.baseURL = baseURL
    }

    func fetchMeetings() async throws -> [Meeting] {
        try await request(path: "get-meetings")
    }

    func fetchMeeting(id: String) async throws -> Meeting {
        try await request(path: "get-meeting/\(id)")
    }

    private func request<Value: Decodable>(path: String) async throws -> Value {
        let url = baseURL.appendingPathComponent(path)
        let (data, response) = try await URLSession.shared.data(from: url)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw MeetilyAPIError.invalidResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw MeetilyAPIError.server(statusCode: httpResponse.statusCode)
        }

        return try JSONDecoder().decode(Value.self, from: data)
    }

    static func savedBaseURL() -> URL {
        if let saved = UserDefaults.standard.string(forKey: "meetily.apiBaseURL"),
           let url = URL(string: saved),
           url.scheme != nil,
           url.host != nil {
            return url
        }

        return URL(string: "http://127.0.0.1:8000")!
    }

    static func saveBaseURL(_ url: URL) {
        UserDefaults.standard.set(url.absoluteString.trimmingCharacters(in: CharacterSet(charactersIn: "/")), forKey: "meetily.apiBaseURL")
    }
}
