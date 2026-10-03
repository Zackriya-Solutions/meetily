# Sottoly

> Your advisory board in every meeting. Pronounced **SOH-toh-lee** (from *sotto voce*, "in a low voice").

🚧 **Building in public toward Claude Code Build Day Bogotá (October 5, 2026).** Nothing here is ready to use yet.

Sottoly is a desktop app that listens to your high-stakes 1:1 meetings (money, contracts, deadlines) and shows you short, live suggestions from a board of AI roles: a CFO who catches hidden costs, an adversarial CEO who questions decisions made on autopilot. It doesn't take notes for later; it helps you decide during the meeting.

- **Local first:** audio is captured and transcribed on your Mac. No audio is stored.
- **Bring your own model:** connect your own API keys.
- **Not invisible:** the overlay shows up when you share your screen, and Sottoly reminds you to tell the other side.
- **Portable roles:** each role is a Markdown + YAML file you can write and share.

## Status

macOS (Apple Silicon) only, MVP in progress. The build plan (in Spanish) is in [SPEC.md](SPEC.md), the vocabulary in [GLOSSARY.md](GLOSSARY.md), and key decisions in [docs/adr/](docs/adr/).

## Why are there unused folders?

Sottoly is a faithful fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) so upstream improvements to capture and transcription can be merged easily. Meetily folders that Sottoly doesn't use (such as `backend/` and `llama-helper/`) are kept but excluded from the build. Sottoly's own code lives in `engine/`, `roles/` and `evals/`. See [ADR-0001](docs/adr/0001-fork-fiel-de-meetily.md).

## License

MIT. See [LICENSE.md](LICENSE.md).

## Credits

- Audio capture and transcription based on [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) (MIT, © Zackriya Solutions).
- Overlay pattern inspired by [Coucou](https://github.com/Louis-CFM/coucou) (MIT).
