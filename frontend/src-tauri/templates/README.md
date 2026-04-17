# 회의 요약 템플릿

이 디렉토리에는 회의 요약 생성을 위한 템플릿 정의가 포함됩니다. 기본 내장 템플릿은 모두 한국어입니다.

## 기본 템플릿

### 1. `ko_standard_meeting.json` — 표준 회의록
일반 회의를 위한 기본 템플릿. 핵심 요약 / 결정 사항 / 실행 항목 / 토론 하이라이트 / 참석자.

### 2. `ko_one_on_one.json` — 1:1 미팅
매니저와 팀원의 1:1 미팅. 상태 공유 / 블로커 / 성장·피드백 / 다음 액션.

### 3. `ko_client_call.json` — 고객 미팅
영업·고객사 미팅. 고객 니즈 / 논의 주제 / 합의 및 이견 / 후속 조치.

### 4. `ko_daily_standup.json` — 데일리 스탠드업
어제 한 일 / 오늘 할 일 / 블로커.

### 5. `ko_retrospective.json` — 회고
Keep / Problem / Try / 액션 아이템.

## 템플릿 구조

각 템플릿 JSON 파일은 다음 스키마를 따릅니다.

```json
{
  "name": "템플릿 이름",
  "description": "템플릿 용도에 대한 짧은 설명",
  "sections": [
    {
      "title": "섹션 제목",
      "instruction": "LLM 지시사항",
      "format": "paragraph|list|string",
      "item_format": "list일 경우 마크다운 표 포맷(선택)"
    }
  ]
}
```

## 커스텀 템플릿

사용자는 앱 데이터 디렉토리에 커스텀 템플릿을 추가할 수 있습니다.

- **macOS**: `~/Library/Application Support/Meetily/templates/`
- **Windows**: `%APPDATA%\Meetily\templates\`
- **Linux**: `~/.config/Meetily/templates/`

같은 파일명을 사용하면 내장 템플릿을 덮어씁니다.

## 필드 설명

### Root
- `name` (required)
- `description` (required)
- `sections` (required)

### Section
- `title` (required)
- `instruction` (required)
- `format` (required): `"paragraph"`, `"list"`, `"string"` 중 하나
- `item_format` (optional): 리스트 항목 마크다운 포맷
- `example_item_format` (optional)

## 코드 사용 예시

```rust
use crate::summary::templates;

let template = templates::get_template("ko_standard_meeting")?;
let available = templates::list_templates();
```
