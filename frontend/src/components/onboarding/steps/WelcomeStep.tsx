import React from 'react';
import { Lock, Sparkles, Cpu } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { ClearMinutesIcon } from '@/components/ClearMinutesLogo';

export function WelcomeStep() {
  const { goNext } = useOnboarding();

  const features = [
    {
      icon: Lock,
      title: 'Your data never leaves your device',
    },
    {
      icon: Sparkles,
      title: 'Intelligent summaries & insights',
    },
    {
      icon: Cpu,
      title: 'Works offline, no cloud required',
    },
  ];

  return (
    <OnboardingContainer
      title=""
      description="Record. Transcribe. Summarize. All on your device."
      step={1}
      hideProgress={true}
    >
      <div className="flex flex-col items-center space-y-10">
        {/* Full-colour logo + wordmark */}
        <div className="flex flex-col items-center gap-3">
          <ClearMinutesIcon size={72} />
          <span
            className="text-2xl font-extrabold tracking-tight text-gray-900 dark:text-foreground"
            style={{ fontFamily: 'var(--font-syne, system-ui)', letterSpacing: '-0.03em' }}
          >
            clearminutes
          </span>
        </div>

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
            className="w-full h-11 text-white hover:opacity-90 transition-opacity"
            style={{ backgroundColor: 'hsl(var(--theme-accent))' }}
          >
            Get Started
          </Button>
          <p className="text-xs text-center text-gray-500">Takes less than 3 minutes</p>
        </div>
      </div>
    </OnboardingContainer>
  );
}
