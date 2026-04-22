import React from 'react';
import { Clock, Users, Calendar, Tag } from 'lucide-react';

interface PageProps {
  params: {
    id: string;
  };
}

interface Note {
  title: string;
  date: string;
  time?: string;
  attendees?: string[];
  tags: string[];
  content: string;
}

export function generateStaticParams() {
  // Return all possible note IDs
  return [
    { id: 'team-sync-dec-26' },
    { id: 'product-review' },
    { id: 'project-ideas' },
    { id: 'action-items' }
  ];
}

const NotePage = ({ params }: PageProps) => {
  // This would normally come from your database
  const sampleData: Record<string, Note> = {
    'team-sync-dec-26': {
      title: '팀 싱크 - 12월 26일',
      date: '2024-12-26',
      time: '오전 10:00 - 오전 11:00',
      attendees: ['John Doe', 'Jane Smith', 'Mike Johnson'],
      tags: ['팀 싱크', '주간', '제품'],
      content: `
# 회의 요약
2024년 1분기 목표 및 현재 프로젝트 현황에 관한 팀 싱크 논의.

## 안건
1. 프로젝트 현황 업데이트
2. 2024년 1분기 계획
3. 팀 우려 사항 및 피드백

## 주요 결정 사항
- 1분기 모바일 앱 개발 우선 추진
- 주간 디자인 리뷰 일정 수립
- 로드맵에 새로운 기능 2개 추가

## 액션 아이템
- [ ] John: 프로젝트 타임라인 작성
- [ ] Jane: 디자인 리뷰 회의 일정 조율
- [ ] Mike: 문서 업데이트

## 노트
- 현재 프로젝트 병목 현상 논의
- 지난 릴리즈에 대한 고객 피드백 검토
- 다가오는 스프린트를 위한 자원 배분 계획
      `
    },
    'product-review': {
      title: '제품 리뷰',
      date: '2024-12-26',
      time: '오후 2:00 - 오후 3:00',
      attendees: ['Sarah Wilson', 'Tom Brown', 'Alex Chen'],
      tags: ['제품', '리뷰', '분기'],
      content: `
# 제품 리뷰 회의

## 개요
이해관계자와의 분기별 제품 리뷰 세션.

## 논의 사항
1. 4분기 실적 리뷰
2. 기능 우선순위 결정
3. 고객 피드백 분석

## 액션 아이템
- [ ] 제품 로드맵 업데이트
- [ ] 사용자 리서치 세션 일정 수립
- [ ] 경쟁사 분석 검토
      `
    },
    'project-ideas': {
      title: '프로젝트 아이디어',
      date: '2024-12-26',
      tags: ['아이디어', '기획'],
      content: `
# 프로젝트 아이디어

## 새로운 기능
1. AI 기반 회의 요약
2. 캘린더 연동
3. 팀 협업 도구

## 개선 사항
- 검색 기능 강화
- 노트 정리 방식 개선
- 실시간 협업
      `
    },
    'action-items': {
      title: '액션 아이템',
      date: '2024-12-26',
      tags: ['작업', '할 일', '기획'],
      content: `
# 액션 아이템

## 높은 우선순위
- [ ] v2.0 프로덕션 배포
- [ ] 심각한 보안 문제 수정
- [ ] 사용자 문서 완성

## 중간 우선순위
- [ ] 의존성 업데이트
- [ ] 오류 추적 구현
- [ ] 단위 테스트 추가

## 낮은 우선순위
- [ ] 레거시 코드 리팩터링
- [ ] 코드 문서 개선
- [ ] 개발 가이드라인 설정
      `
    }
  };

  const note = sampleData[params.id as keyof typeof sampleData];

  if (!note) {
    return <div className="p-8">노트를 찾을 수 없습니다</div>;
  }

  return (
    <div className="p-8 max-w-4xl mx-auto">
      <div className="mb-8">
        <h1 className="text-3xl font-bold mb-4">{note.title}</h1>
        
        <div className="flex flex-wrap gap-4 text-gray-600">
          {note.date && (
            <div className="flex items-center gap-1">
              <Calendar className="w-4 h-4" />
              <span>{note.date}</span>
            </div>
          )}
          
          {note.time && (
            <div className="flex items-center gap-1">
              <Clock className="w-4 h-4" />
              <span>{note.time}</span>
            </div>
          )}
          
          {note.attendees && (
            <div className="flex items-center gap-1">
              <Users className="w-4 h-4" />
              <span>{note.attendees.join(', ')}</span>
            </div>
          )}
        </div>

        <div className="flex gap-2 mt-4">
          {note.tags.map((tag) => (
            <div key={tag} className="flex items-center gap-1 bg-blue-100 text-blue-800 px-2 py-1 rounded-full text-sm">
              <Tag className="w-3 h-3" />
              {tag}
            </div>
          ))}
        </div>
      </div>

      <div className="prose prose-blue max-w-none">
        <div dangerouslySetInnerHTML={{ __html: note.content.split('\n').map(line => {
          if (line.startsWith('# ')) {
            return `<h1>${line.slice(2)}</h1>`;
          } else if (line.startsWith('## ')) {
            return `<h2>${line.slice(3)}</h2>`;
          } else if (line.startsWith('- ')) {
            return `<li>${line.slice(2)}</li>`;
          }
          return line;
        }).join('\n') }} />
      </div>
    </div>
  );
};

export default NotePage;
