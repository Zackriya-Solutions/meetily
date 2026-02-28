# Implementation Plan - Restore Dynamic Prompts

The goal is to re-integrate the dynamic meeting context prompt (Title, Description, Participants) into the `main` branch, as this was the successful part of the previous experiment.

## User Review Required

> [!IMPORTANT]
> This plan only restores the **Dynamic Prompting** feature.
> It deliberately **omits**:
> *   Server-side noise reduction (disabled)
> *   VAD buffering changes (reverted to original)
> *   Aggressive deduplication logic (reverted to original)
> *   Hallucination filtering changes (reverted to original)

## Proposed Changes

### 1. Backend: Audio Router (`backend/app/api/routers/audio.py`)

**Update `websocket_streaming_audio` function:**
*   **Fetch Context:** Before initializing the manager, fetch the meeting details using `db.get_meeting(active_meeting_id)`.
*   **Extract Data:** Create a `meeting_context` dictionary containing:
    *   `title`
    *   `description`
    *   `participants` (list of names)
*   **Pass to Manager:** Update the `StreamingTranscriptionManager` instantiation to pass this context.

```python
# Pseudo-code for insertion
meeting_context = {}
if active_meeting_id:
    try:
        meeting_data = await db.get_meeting(active_meeting_id)
        if meeting_data:
            meeting_context = {
                "title": meeting_data.get("title"),
                "description": meeting_data.get("description"),
                "participants": meeting_data.get("participants", [])
            }
    except Exception as e:
        logger.warning(f"Failed to fetch context: {e}")

manager = StreamingTranscriptionManager(groq_api_key, meeting_context)
```

### 2. Backend: Transcription Manager (`backend/app/services/audio/manager.py`)

**Update `StreamingTranscriptionManager` class:**
*   **`__init__`**: Accept `meeting_context` as an optional argument and store it.
*   **`_construct_prompt`**: Add a helper method to generate the system prompt.
    *   *Base:* "This is a business meeting."
    *   *Additions:* "Title: {title}", "Topic: {description}", "Participants: {names}".
    *   *Context:* Append the last few words of the previous transcript for continuity (existing feature, but integrated into the new prompt builder).
*   **`process_audio_chunk`**: Update the `groq.transcribe_audio_async` call to use `self._construct_prompt()` instead of the static prompt.

## Verification Plan

### Automated Tests
*   Since this logic depends on live DB data and Groq API calls, unit tests are limited without mocks.
*   I will verify the code syntax and structure after edits.

### Manual Verification (User)
*   Start a meeting with a known Title and Description.
*   Speak into the microphone.
*   The transcription quality for specific terms in the agenda/title should be improved (though subtle to detect without side-by-side comparison).
*   **Crucially:** Verify that the "bad" audio artifacts from the previous branch (e.g., cut-off words, aggressive filtering) are gone.
