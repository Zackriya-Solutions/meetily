import React, { useState, useEffect } from 'react';
import { Globe } from 'lucide-react';
import Analytics from '@/lib/analytics';
import { toast } from 'sonner';
import { useConfig } from '@/contexts/ConfigContext';

export interface Language {
  code: string;
  name: string;
}

// ISO 639-1 language codes supported by Whisper
const LANGUAGES: Language[] = [
  { code: 'auto', name: '자동 감지 (원본 언어)' },
  { code: 'auto-translate', name: '자동 감지 (영어로 번역)' },
  { code: 'en', name: '영어' },
  { code: 'zh', name: '중국어' },
  { code: 'de', name: '독일어' },
  { code: 'es', name: '스페인어' },
  { code: 'ru', name: '러시아어' },
  { code: 'ko', name: '한국어' },
  { code: 'fr', name: '프랑스어' },
  { code: 'ja', name: '일본어' },
  { code: 'pt', name: '포르투갈어' },
  { code: 'tr', name: '터키어' },
  { code: 'pl', name: '폴란드어' },
  { code: 'ca', name: '카탈루냐어' },
  { code: 'nl', name: '네덜란드어' },
  { code: 'ar', name: '아랍어' },
  { code: 'sv', name: '스웨덴어' },
  { code: 'it', name: '이탈리아어' },
  { code: 'id', name: '인도네시아어' },
  { code: 'hi', name: '힌디어' },
  { code: 'fi', name: '핀란드어' },
  { code: 'vi', name: '베트남어' },
  { code: 'he', name: '히브리어' },
  { code: 'uk', name: '우크라이나어' },
  { code: 'el', name: '그리스어' },
  { code: 'ms', name: '말레이어' },
  { code: 'cs', name: '체코어' },
  { code: 'ro', name: '루마니아어' },
  { code: 'da', name: '덴마크어' },
  { code: 'hu', name: '헝가리어' },
  { code: 'ta', name: '타밀어' },
  { code: 'no', name: '노르웨이어' },
  { code: 'th', name: '태국어' },
  { code: 'ur', name: '우르두어' },
  { code: 'hr', name: '크로아티아어' },
  { code: 'bg', name: '불가리아어' },
  { code: 'lt', name: '리투아니아어' },
  { code: 'la', name: '라틴어' },
  { code: 'mi', name: '마오리어' },
  { code: 'ml', name: '말라얄람어' },
  { code: 'cy', name: '웨일스어' },
  { code: 'sk', name: '슬로바키아어' },
  { code: 'te', name: '텔루구어' },
  { code: 'fa', name: '페르시아어' },
  { code: 'lv', name: '라트비아어' },
  { code: 'bn', name: '벵골어' },
  { code: 'sr', name: '세르비아어' },
  { code: 'az', name: '아제르바이잔어' },
  { code: 'sl', name: '슬로베니아어' },
  { code: 'kn', name: '칸나다어' },
  { code: 'et', name: '에스토니아어' },
  { code: 'mk', name: '마케도니아어' },
  { code: 'br', name: '브르타뉴어' },
  { code: 'eu', name: '바스크어' },
  { code: 'is', name: '아이슬란드어' },
  { code: 'hy', name: '아르메니아어' },
  { code: 'ne', name: '네팔어' },
  { code: 'mn', name: '몽골어' },
  { code: 'bs', name: '보스니아어' },
  { code: 'kk', name: '카자흐어' },
  { code: 'sq', name: '알바니아어' },
  { code: 'sw', name: '스와힐리어' },
  { code: 'gl', name: '갈리시아어' },
  { code: 'mr', name: '마라티어' },
  { code: 'pa', name: '펀자브어' },
  { code: 'si', name: '싱할라어' },
  { code: 'km', name: '크메르어' },
  { code: 'sn', name: '쇼나어' },
  { code: 'yo', name: '요루바어' },
  { code: 'so', name: '소말리어' },
  { code: 'af', name: '아프리칸스어' },
  { code: 'oc', name: '오크어' },
  { code: 'ka', name: '조지아어' },
  { code: 'be', name: '벨라루스어' },
  { code: 'tg', name: '타지크어' },
  { code: 'sd', name: '신드어' },
  { code: 'gu', name: '구자라트어' },
  { code: 'am', name: '암하라어' },
  { code: 'yi', name: '이디시어' },
  { code: 'lo', name: '라오어' },
  { code: 'uz', name: '우즈베크어' },
  { code: 'fo', name: '페로어' },
  { code: 'ht', name: '아이티 크리올어' },
  { code: 'ps', name: '파슈토어' },
  { code: 'tk', name: '투르크멘어' },
  { code: 'nn', name: '노르웨이어 (뉘노르스크)' },
  { code: 'mt', name: '몰타어' },
  { code: 'sa', name: '산스크리트어' },
  { code: 'lb', name: '룩셈부르크어' },
  { code: 'my', name: '미얀마어' },
  { code: 'bo', name: '티베트어' },
  { code: 'tl', name: '타갈로그어' },
  { code: 'mg', name: '말라가시어' },
  { code: 'as', name: '아삼어' },
  { code: 'tt', name: '타타르어' },
  { code: 'haw', name: '하와이어' },
  { code: 'ln', name: '링갈라어' },
  { code: 'ha', name: '하우사어' },
  { code: 'ba', name: '바시키르어' },
  { code: 'jw', name: '자바어' },
  { code: 'su', name: '순다어' },
];

interface LanguageSelectionProps {
  selectedLanguage: string;
  onLanguageChange: (language: string) => void;
  disabled?: boolean;
  provider?: 'cohere';
}

export function LanguageSelection({
  selectedLanguage,
  onLanguageChange,
  disabled = false,
  provider = 'cohere'
}: LanguageSelectionProps) {
  const [saving, setSaving] = useState(false);
  const { setSelectedLanguage } = useConfig();

  const availableLanguages = LANGUAGES;

  const handleLanguageChange = async (languageCode: string) => {
    setSaving(true);
    try {
      // Save language preference to localStorage and sync to backend
      setSelectedLanguage(languageCode);
      onLanguageChange(languageCode);
      console.log('Language preference saved:', languageCode);

      // Track language selection analytics
      const selectedLang = LANGUAGES.find(lang => lang.code === languageCode);
      await Analytics.track('language_selected', {
        language_code: languageCode,
        language_name: selectedLang?.name || 'Unknown',
        is_auto_detect: (languageCode === 'auto').toString(),
        is_auto_translate: (languageCode === 'auto-translate').toString()
      });

      // Show success toast
      const languageName = selectedLang?.name || languageCode;
      toast.success("언어 환경 설정이 저장되었습니다", {
        description: `전사 언어가 ${languageName}(으)로 설정되었습니다`
      });
    } catch (error) {
      console.error('Failed to save language preference:', error);
      toast.error("언어 환경 설정을 저장하지 못했습니다", {
        description: error instanceof Error ? error.message : String(error)
      });
    } finally {
      setSaving(false);
    }
  };

  // Find the selected language name for display. Default to Korean.
  const selectedLanguageName = LANGUAGES.find(
    lang => lang.code === selectedLanguage
  )?.name || '한국어';

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Globe className="h-4 w-4 text-gray-600" />
          <h4 className="text-sm font-medium text-gray-900">전사 언어</h4>
        </div>
      </div>

      <div className="space-y-2">
        <select
          value={selectedLanguage}
          onChange={(e) => handleLanguageChange(e.target.value)}
          disabled={disabled || saving}
          className="w-full px-3 py-2 text-sm bg-white border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500 disabled:bg-gray-50 disabled:text-gray-500"
        >
          {availableLanguages.map((language) => (
            <option key={language.code} value={language.code}>
              {language.name}
              {language.code !== 'auto' && language.code !== 'auto-translate' && ` (${language.code})`}
            </option>
          ))}
        </select>

        {/* Info text */}
        <div className="text-xs space-y-2 pt-2">
          <p className="text-gray-600">
            <strong>현재 언어:</strong> {selectedLanguageName}
          </p>
          {selectedLanguage === 'auto' && (
            <div className="p-2 bg-yellow-50 border border-yellow-200 rounded text-yellow-800">
              <p className="font-medium">⚠️ 자동 감지는 부정확한 결과가 나올 수 있습니다</p>
              <p className="mt-1">정확도를 높이려면 사용할 언어를 직접 선택하세요 (예: 한국어, 영어 등).</p>
            </div>
          )}
          {selectedLanguage === 'auto-translate' && (
            <div className="p-2 bg-blue-50 border border-blue-200 rounded text-blue-800">
              <p className="font-medium">🌐 번역 모드 활성화</p>
              <p className="mt-1">모든 오디오가 자동으로 영어로 번역됩니다. 다국어 회의에서 영어 결과물이 필요할 때 적합합니다.</p>
            </div>
          )}
          {selectedLanguage !== 'auto' && selectedLanguage !== 'auto-translate' && (
            <p className="text-gray-600">
              <strong>{selectedLanguageName}</strong>에 맞춰 전사가 최적화됩니다
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
