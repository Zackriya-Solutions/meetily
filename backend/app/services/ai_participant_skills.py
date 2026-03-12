import re
from pathlib import Path
from typing import Dict, List, Optional


SKILLS_DIR = Path(__file__).resolve().parent.parent / "skills" / "ai_participant"
SYSTEM_SKILL_FILES = {
    "facilitator": "facilitator.md",
    "advisor": "advisor.md",
    "chairperson": "chairperson.md",
}

DEFAULT_ALLOWED_CUSTOM_EVENT_TYPES = [
    "follow_up_needed",
    "blocker_detected",
    "next_step_clarified",
]


def get_system_skill_markdown(skill_name: str) -> str:
    filename = SYSTEM_SKILL_FILES.get(str(skill_name or "").strip().lower())
    if not filename:
        return ""
    path = SKILLS_DIR / filename
    try:
        return path.read_text(encoding="utf-8").strip()
    except FileNotFoundError:
        return ""


def load_system_skill_templates() -> Dict[str, str]:
    return {
        skill_name: markdown
        for skill_name, markdown in (
            (name, get_system_skill_markdown(name)) for name in SYSTEM_SKILL_FILES
        )
        if markdown
    }


def parse_skill_markdown(skill_markdown: str) -> Dict[str, object]:
    text = str(skill_markdown or "").strip()
    meta = _parse_frontmatter(text)
    allowed_types = _parse_allowed_custom_event_types(text)
    rules = _parse_bulleted_section(text, "Rules")
    goals = _parse_bulleted_section(text, "Goals")
    role = _parse_section_body(text, "Role")

    return {
        "name": str(meta.get("name") or "").strip(),
        "description": str(meta.get("description") or "").strip(),
        "role": role,
        "goals": goals,
        "rules": rules,
        "allowed_custom_event_types": allowed_types or list(DEFAULT_ALLOWED_CUSTOM_EVENT_TYPES),
    }


def _parse_frontmatter(text: str) -> Dict[str, str]:
    match = re.match(r"^\s*---\s*\n(.*?)\n---\s*\n?", text, flags=re.DOTALL)
    if not match:
        return {}

    meta: Dict[str, str] = {}
    for raw_line in match.group(1).splitlines():
        line = raw_line.strip()
        if not line or ":" not in line:
            continue
        key, value = line.split(":", 1)
        clean_key = key.strip().lower()
        clean_value = value.strip().strip('"').strip("'")
        if clean_key:
            meta[clean_key] = clean_value
    return meta


def _parse_section_body(text: str, section_name: str) -> str:
    pattern = rf"(?im)^#{{1,6}}\s+{re.escape(section_name)}\s*$"
    match = re.search(pattern, text)
    if not match:
        return ""

    start = match.end()
    remainder = text[start:]
    next_heading = re.search(r"(?im)^#{1,6}\s+", remainder)
    body = remainder[: next_heading.start()] if next_heading else remainder
    return body.strip()


def _parse_bulleted_section(text: str, section_name: str) -> List[str]:
    body = _parse_section_body(text, section_name)
    if not body:
        return []

    items: List[str] = []
    for raw_line in body.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if re.match(r"^[-*]\s+", line):
            items.append(re.sub(r"^[-*]\s+", "", line).strip())
            continue
        if re.match(r"^\d+\.\s+", line):
            items.append(re.sub(r"^\d+\.\s+", "", line).strip())
    return items


def _parse_allowed_custom_event_types(text: str) -> List[str]:
    items = _parse_bulleted_section(text, "Allowed Custom Event Types")
    allowed: List[str] = []
    for item in items:
        match = re.search(r"`([^`]+)`", item)
        candidate = match.group(1) if match else item.split(":", 1)[0]
        normalized = _normalize_event_type(candidate)
        if normalized and normalized not in allowed:
            allowed.append(normalized)
    return allowed


def _normalize_event_type(value: Optional[str]) -> str:
    raw = str(value or "").strip().lower()
    raw = raw.replace("-", "_").replace(" ", "_")
    raw = re.sub(r"[^a-z0-9_]", "", raw)
    raw = re.sub(r"_+", "_", raw).strip("_")
    return raw
