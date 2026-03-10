import sys
import re


def patch_audio_recorder():
    path = "backend/app/services/audio/recorder.py"
    with open(path, "r") as f:
        content = f.read()

    # We need to remove the early return in merge_chunks so it actually merges the chunks.
    # Since we're doing append-based merging in post_recording, AudioRecorder should just return the chunk data.

    old_block = """            # Check for existing merged files first
            merged_pcm = chunk_dir / "merged_recording.pcm"
            if merged_pcm.exists():
                logger.info(f"Found existing merged PCM file: {merged_pcm}")
                async with aiofiles.open(merged_pcm, "rb") as f:
                    return await f.read()

            merged_wav = chunk_dir / "merged_recording.wav"
            if merged_wav.exists():
                logger.info(f"Found existing merged WAV file: {merged_wav}")
                async with aiofiles.open(merged_wav, "rb") as f:
                    return await f.read()"""

    new_block = """            # Only merge actual chunks so we can append them to the final recording.wav in post_recording
            # (Removed early return of merged_recording to prevent ignoring new chunks)"""

    if old_block in content:
        content = content.replace(old_block, new_block)
        with open(path, "w") as f:
            f.write(content)
        print("Updated audio/recorder.py")
    else:
        print("Could not find the block in recorder.py (might be already patched)")


patch_audio_recorder()
