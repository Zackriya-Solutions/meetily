import React, { useEffect, useState } from 'react';
import { Info } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { formatFileSize, getModelDisplayName } from '@/lib/parakeet';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

export function SetupOverviewStep() {
  const {
    transcriptionModels, selectedTranscriptionModel, loadTranscriptionModels, beginDownloads,
  } = useOnboarding();
  const [choice, setChoice] = useState(selectedTranscriptionModel);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [isMac, setIsMac] = useState(false);

  useEffect(() => {
    const checkPlatform = async () => {
      try {
        const { platform } = await import('@tauri-apps/plugin-os');
        setIsMac(platform() === 'macos');
      } catch (e) {
        setIsMac(navigator.userAgent.includes('Mac'));
      }
    };
    checkPlatform();
  }, []);

  const steps = [
    {
      number: 1,
      type: 'transcription',
      title: 'Download Transcription Engine',
    },
    {
      number: 2,
      type: 'summarization',
      title: 'Download Summarization Engine',
    },
  ];

  useEffect(() => {
    setChoice(selectedTranscriptionModel);
  }, [selectedTranscriptionModel]);

  const handleContinue = async () => {
    if (pending) return;
    setPending(true);
    setError('');
    try {
      if (transcriptionModels) await beginDownloads(choice);
      else await loadTranscriptionModels();
    } catch (error) {
      setError(String(error));
    } finally {
      setPending(false);
    }
  };

  return (
    <OnboardingContainer
      title="Setup Overview"
      description="Meetily requires that you download the Transcription & Summarization AI models for the software to work."
      step={2}
      totalSteps={isMac ? 4 : 3}
    >
      <div className="flex flex-col items-center space-y-10">
        {/* Steps Card */}
        <div className="w-full max-w-md bg-white rounded-lg border border-gray-200 p-4">
          <div className="space-y-4">
            {steps.map((step, idx) => {
              return (
                <div
                  key={step.number}
                  className="p-1"
                >
                  <div className="flex-1 ml-1">
                    <h3 className="font-medium text-gray-900 flex items-center gap-2">
                        Step {step.number} :  {step.title}

                        {step.type === "summarization" && (
                            <TooltipProvider>
                            <Tooltip>
                                <TooltipTrigger asChild>
                                <button className="text-gray-400 hover:text-gray-600">
                                    <Info className="w-4 h-4" />
                                </button>
                                </TooltipTrigger>
                                <TooltipContent className="max-w-xs text-sm">
                                You can also select external AI providers like OpenAI, Claude, or
                                Ollama for summary generation in settings.
                                </TooltipContent>
                            </Tooltip>
                            </TooltipProvider>
                        )}
                        </h3>
                  </div>
                  {step.type === 'transcription' && transcriptionModels && transcriptionModels.length > 1 && (
                    <label className="flex flex-col gap-1.5 text-sm text-gray-700 mt-2">
                      Transcription model
                      <select
                        className="rounded-md border border-gray-300 bg-white px-3 py-2"
                        value={choice}
                        onChange={event => setChoice(event.target.value)}
                        disabled={pending}
                      >
                        {transcriptionModels.map(model => (
                          <option key={model.name} value={model.name}>
                            {getModelDisplayName(model.name)} · {formatFileSize(model.size_mb)}
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                </div>
              );
            })}
          </div>
        </div>


        {/* CTA Section */}
        <div className="w-full max-w-xs space-y-4">
          {error && <p role="alert" className="text-sm text-red-700">{error}</p>}
          <Button
            onClick={handleContinue}
            disabled={pending}
            className="w-full h-11 bg-gray-900 hover:bg-gray-800 text-white"
          >
            {transcriptionModels ? "Let's Go" : 'Check transcription models'}
          </Button>
          <div className="text-center">
            <a
              href="https://github.com/Zackriya-Solutions/meeting-minutes"
              target="_blank"
              rel="noopener noreferrer"
              className="text-xs text-gray-600 hover:underline"
            >
              Report issues on GitHub
            </a>
          </div>
        </div>
      </div>
    </OnboardingContainer>
  );
}
