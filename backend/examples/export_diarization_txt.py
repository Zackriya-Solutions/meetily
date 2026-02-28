import json
import sys


def main():
    try:
        with open("combined_alignment_benchmark.json", "r") as f:
            data = json.load(f)

        segments = data.get("sample", {}).get("compacted_aligned_first_20", [])

        with open("diarized_output.txt", "w") as f:
            for seg in segments:
                start = seg.get("start", 0.0)
                end = seg.get("end", 0.0)
                speaker = seg.get("speaker", "Unknown")
                text = seg.get("text", "")

                # Format: [00:00 - 00:05] Speaker 1: Hello world
                start_min = int(start // 60)
                start_sec = int(start % 60)
                end_min = int(end // 60)
                end_sec = int(end % 60)

                line = f"[{start_min:02d}:{start_sec:02d} - {end_min:02d}:{end_sec:02d}] {speaker}: {text}\n"
                f.write(line)

        print("Successfully exported to diarized_output.txt")

    except Exception as e:
        print(f"Error: {e}")


if __name__ == "__main__":
    main()
