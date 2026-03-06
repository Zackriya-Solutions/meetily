# Meeting Notes Sharing & Collaboration Refactoring

## 1. Objective
Refactor the note-sharing workflow to improve user experience, reduce email clutter, and drive collaborative engagement directly within the Meeting Co-Pilot application. 

## 2. Current Pain Points
- Emails contain truncated meeting notes and irrelevant Google Meet links.
- Re-generating notes automatically re-sends emails, leading to inbox spam.
- Users have no centralized hub within the app to view notes that have been shared with them.
- Ad-hoc meetings uselessly trigger sharing flows even without attendees.

## 3. Proposed Core Changes

### 3.1 Email Redesign: Link-First Approach
Instead of sending the raw, truncated notes in the email body, the email will serve as a gateway to the app.
- **Actionable Link**: Provide a clear "View Full Meeting Notes & Transcript" button that takes them directly to the meeting notes page within the app.
- **Value-Add Snippet**: To avoid frustrating users who just want a quick glance, we should include a 1-sentence TL;DR or the top 3 Action Items in the email itself. Everything else is accessed via the link.
- **Remove Irrelevant Links**: Stop appending the Google Meet link to the email.

### 3.2 The "Shared Notes" Page & App Experience
When an attendee clicks the email link, they are directed to the full meeting page.
- **Feature Parity**: They should have access to the robust interface, including:
  - Full AI Summary
  - Searchable Transcript
  - Download/Export functionality
  - Refine options (AI chat / Q&A for the meeting)
- **New Sidebar Tab ("Shared with Me")**: For users who are already on the platform, introduce a "Shared Notes" tab in the main sidebar. This aggregates all meetings where the user was an attendee but not the host.

### 3.3 Smart Re-generation & Sharing Flow
Regenerating an AI summary should never result in an automatic email blast.
- **Manual Prompt on Regeneration**: When a user refines or regenerates the notes, present a prompt or toggle: `"Share updated notes with all attendees?"`. 
- **Ad-Hoc Meeting Detection**: If the meeting was spontaneous (not linked to a calendar event, or has 0 external attendees), **skip the sharing prompt entirely**. The system should silently save the regeneration.
- **Smart Recipient Filtering**: Only send the notes email to attendees who explicitly accepted (or tentatively accepted) the meeting invite. Do not share notes with users who declined or did not RSVP.

---

## 4. Suggested Additions (Making it more functional & less frustrating)

To elevate this feature beyond the basic requirements, here are a few recommended additions from an engineering and UX perspective to make it less frustrating and more powerful:

### 4.1 Sign-up to View & Calendar Integration (Acquisition Strategy)
- **Required Authentication:** To view the full meeting notes, attendees must log in or create an account. This is a critical acquisition loop.
- **Onboarding & Calendar Connection:** During the sign-up flow triggered by a shared notes link, the primary onboarding step should be connecting their calendar (Google/Outlook). 
- **Immediate Value Add:** By connecting their calendar immediately, the new user will start receiving Meeting Co-Pilot emails for their own upcoming meetings, seamlessly converting them from a passive reader to an active user.

### 4.2 Granular Sharing Toggles
- Before sending the initial email (or in the settings), allow the owner to select *what* gets shared. (e.g., "Share Summary" ✅, "Share Transcript" ❌). Sometimes transcripts contain candid banter that the host may not want to distribute broadly, even if the summary is safe.

### 4.3 "Silent Updates" vs "Notify Attendees"
- **Update Badges**: If the owner chooses *not* to email attendees after a regeneration, the app simply updates the "Shared Notes" view for those users. When they next visit the app, a small "Updated" badge can appear next to that meeting in their sidebar.

### 4.4 Personal vs. Global Refinements
- If attendees have access to the "Refine" tool on the shared page, we must decide if their refinements overwrite the main note for *everyone*, or just for themselves. 
- **Recommendation**: Make the "Refine" chat personal to the viewing user, so an attendee asking the AI a question doesn't alter the official meeting summary for everyone else.

### 4.5 Batch Sharing (Future feature)
- If a user attends 5 meetings in a day, sending 5 separate emails might be annoying. A future iteration could include a "Your Meeting Summaries for Today" digest sent at the end of the day.