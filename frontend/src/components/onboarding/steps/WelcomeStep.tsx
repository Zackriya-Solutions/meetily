import React from 'react';
import { Lock, Sparkles, Cpu } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';

export function WelcomeStep() {
  const { goNext } = useOnboarding();

  const features = [
    {
      icon: Lock,
      title: '데이터는 기기 밖으로 나가지 않습니다',
    },
    {
      icon: Sparkles,
      title: '지능형 요약과 인사이트',
    },
    {
      icon: Cpu,
      title: '클라우드 없이 오프라인으로 동작합니다',
    },
  ];

  return (
    <OnboardingContainer
      title="Meetily에 오신 것을 환영합니다"
      description="녹음하고, 전사하고, 요약합니다. 모두 이 기기에서."
      step={1}
      hideProgress={true}
    >
      <div className="flex flex-col items-center space-y-10">
        {/* Divider */}
        <div className="w-16 h-px bg-gray-300" />

        {/* Features Card */}
        <div className="w-full max-w-md bg-white rounded-lg border border-gray-200 shadow-sm p-6 space-y-4">
          {features.map((feature, index) => {
            const Icon = feature.icon;
            return (
              <div key={index} className="flex items-start gap-3">
                <div className="flex-shrink-0 mt-0.5">
                  <div className="w-5 h-5 rounded-full bg-gray-100 flex items-center justify-center">
                    <Icon className="w-3 h-3 text-gray-700" />
                  </div>
                </div>
                <p className="text-sm text-gray-700 leading-relaxed">{feature.title}</p>
              </div>
            );
          })}
        </div>

        {/* CTA Section */}
        <div className="w-full max-w-xs space-y-3">
          <Button
            onClick={goNext}
            className="w-full h-11 bg-gray-900 hover:bg-gray-800 text-white"
          >
            시작하기
          </Button>
          <p className="text-xs text-center text-gray-500">3분이면 끝납니다</p>
        </div>
      </div>
    </OnboardingContainer>
  );
}
