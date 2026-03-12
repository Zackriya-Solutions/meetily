# BUG: Open Discussion Over-Generation & Duplication

## Status
Confirmed - Awaiting AI Participant Rebuild (Architecture V2)

## Description
The current AI Host system generates a massive, repetitive list of "Open Discussions" during active meetings. 
The items are not sorted chronologically, they are highly verbose, and semantically identical debates are logged multiple times.

### Example Output (Observed)
```text
The debate has become increasingly intense, with participants acknowledging that the conversation has turned into 'roasting.' This high friction may prevent the group from reaching a constructive consensus on the topic.
The group is yet to resolve whether a freelancer who asks 'why' and focuses on outcomes is fundamentally the same as a product engineer, or if their methods of acquiring tasks differ entirely.
The discussion has become increasingly tense with participants interrupting and 'roasting' each other over the definition of professional roles and levels of responsibility.
The group is questioning the specific distinction between accountability and responsibility as it pertains to a freelancer's commitment to a project's success versus just completing a task.
A participant raised a question regarding the specific differences in customer obsession and role responsibilities between a Product Engineer and a Forward Deployed Engineer.
The participants are seeking a clear definition of a freelancer, specifically debating whether they act as product engineers or merely execute pre-defined tasks for clients.
A participant has asked for a clearer definition of what a 'freelancer' entails when compared to structured engineering roles like Product Engineering.
```

## Root Cause Analysis

1. **Rolling Window Duplication:**
   The `AIParticipantEngine` evaluates a sliding window of the transcript every few seconds. If a debate about "Freelancers vs Product Engineers" lasts for 5 minutes, the LLM analyzes that exact same debate in Window 1, Window 2, Window 3, etc. Every time, it fires a brand new `open_question` or `conflict_risk` event, causing the UI to fill up with duplicates.

2. **Lack of Deduplication & State:**
   The backend relies on exact string matching or very basic hashing to deduplicate events. Because LLMs phrase the exact same observation slightly differently each time (e.g., "A participant has asked for a clearer definition..." vs "The participants are seeking a clear definition..."), the deduplication logic fails.

3. **Verbosity (Prompt Engineering):**
   The prompt does not strictly enforce conciseness for Open Discussions. Instead of a tight UI badge like "Debate: Freelancer Definition," it outputs a full paragraph of analysis.

4. **UI Sorting:**
   The frontend UI (`frontend/src/app/page.tsx`) simply appends these items to an array or sorts them based on backend suggestion ID logic, which leads to a chaotic, out-of-order user experience.

## The Fix (Tied to the AI Participant Rebuild)

This issue proves exactly why the current system is overly complicated and brittle, and why the "Structured Core" rebuild detailed in `pnyx-docs/architecture/AI_PARTICIPANT_ARCHITECTURE.md` is necessary.

### Expected Behavior After Rebuild
When rebuilding the `open_discussion` extraction from scratch, we must implement the following:

1. **Stateful Topic Tracking:**
   Instead of generating isolated events, the LLM prompt must be given the *current list of open discussions* and asked: 
   *"Does the recent transcript introduce a NEW open discussion, or does it belong to one of the existing topics?"* 
   If it belongs to an existing topic, it should do nothing (or update the existing topic's urgency).

2. **Strict Formatting (Titles, not Paragraphs):**
   The prompt for `open_discussion` must mandate a short, 3-to-5 word title (e.g., `title: "Freelancer vs Product Engineer"`) and a slightly longer 1-sentence context string. The UI should only display the title prominently.

3. **Chronological UI Sorting:**
   The frontend must sort `open_discussion` objects by their `timestamp` field in descending order (newest debates at the top), or group them neatly so the UI doesn't become a wall of text.

4. **Resolution Mechanics:**
   The AI Participant must have a way to emit an event that *closes* an open discussion if the transcript shows the group reached an agreement.