import SwiftUI

struct MeetingDetailView: View {
    let meeting: Meeting
    let apiClient: MeetilyAPIClient

    @State private var details: Meeting?
    @State private var isLoading = false
    @State private var errorMessage: String?

    private var displayedMeeting: Meeting {
        details ?? meeting
    }

    var body: some View {
        Group {
            if isLoading && details == nil {
                ProgressView("Loading meeting…")
            } else if let errorMessage, details == nil {
                ContentUnavailableView {
                    Label("Couldn’t load meeting", systemImage: "exclamationmark.triangle")
                } description: {
                    Text(errorMessage)
                } actions: {
                    Button("Try again") {
                        Task { await load() }
                    }
                    .buttonStyle(.borderedProminent)
                }
            } else {
                meetingContent
            }
        }
        .navigationTitle(displayedMeeting.title)
        .navigationBarTitleDisplayMode(.inline)
        .task { await load() }
    }

    private var meetingContent: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 20) {
                if let createdAt = displayedMeeting.createdAt {
                    Label(createdAt, systemImage: "calendar")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }

                VStack(alignment: .leading, spacing: 12) {
                    Text("Transcript")
                        .font(.title3.weight(.semibold))

                    if let transcripts = displayedMeeting.transcripts, !transcripts.isEmpty {
                        ForEach(transcripts, id: \.stableID) { transcript in
                            Text(transcript.text)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(14)
                                .background(Color.gray.opacity(0.12))
                                .clipShape(RoundedRectangle(cornerRadius: 14))
                        }
                    } else {
                        ContentUnavailableView("No transcript yet", systemImage: "text.quote")
                    }
                }
            }
            .padding()
        }
        .refreshable { await load() }
    }

    @MainActor
    private func load() async {
        guard !isLoading else { return }
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            details = try await apiClient.fetchMeeting(id: meeting.id)
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
