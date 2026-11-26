import SwiftUI

struct MeetingListView: View {
    @StateObject private var viewModel: MeetingsViewModel
    @State private var showingSettings = false

    init(apiClient: MeetilyAPIClient = MeetilyAPIClient()) {
        _viewModel = StateObject(wrappedValue: MeetingsViewModel(apiClient: apiClient))
    }

    var body: some View {
        NavigationStack {
            Group {
                if viewModel.isLoading && viewModel.meetings.isEmpty {
                    ProgressView("Loading meetings…")
                } else if let errorMessage = viewModel.errorMessage, viewModel.meetings.isEmpty {
                    ContentUnavailableView {
                        Label("Can’t reach Meetily", systemImage: "wifi.exclamationmark")
                    } description: {
                        Text(errorMessage)
                    } actions: {
                        Button("Try again") {
                            Task { await viewModel.load() }
                        }
                        .buttonStyle(.borderedProminent)
                    }
                } else if viewModel.meetings.isEmpty {
                    ContentUnavailableView("No meetings yet", systemImage: "rectangle.stack")
                } else {
                    List(viewModel.meetings) { meeting in
                        NavigationLink(value: meeting) {
                            MeetingRow(meeting: meeting)
                        }
                    }
                    .listStyle(.insetGrouped)
                    .refreshable { await viewModel.load() }
                }
            }
            .navigationTitle("Meetings")
            .navigationDestination(for: Meeting.self) { meeting in
                MeetingDetailView(meeting: meeting, apiClient: viewModel.apiClient)
            }
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button {
                        showingSettings = true
                    } label: {
                        Label("Settings", systemImage: "gearshape")
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        Task { await viewModel.load() }
                    } label: {
                        Label("Refresh", systemImage: "arrow.clockwise")
                    }
                    .disabled(viewModel.isLoading)
                }
            }
            .sheet(isPresented: $showingSettings) {
                APISettingsView {
                    Task { await viewModel.load() }
                }
            }
        }
        .task { await viewModel.load() }
    }
}

private struct MeetingRow: View {
    let meeting: Meeting

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(meeting.title)
                .font(.headline)
                .lineLimit(2)

            if let createdAt = meeting.createdAt {
                Text(createdAt)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 5)
    }
}
