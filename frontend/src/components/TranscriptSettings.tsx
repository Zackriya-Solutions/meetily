import { Label } from './ui/label';
import { CohereModelManager } from './CohereModelManager';

export interface TranscriptModelProps {
    provider: 'cohere';
    model: string;
    apiKey?: string | null;
}

export interface TranscriptSettingsProps {
    transcriptModelConfig: TranscriptModelProps;
    setTranscriptModelConfig: (config: TranscriptModelProps) => void;
    onModelSelect?: () => void;
}

export function TranscriptSettings({ transcriptModelConfig, setTranscriptModelConfig, onModelSelect }: TranscriptSettingsProps) {
    const handleCohereModelSelect = (modelName: string) => {
        setTranscriptModelConfig({
            provider: 'cohere',
            model: modelName,
            apiKey: null,
        });
        if (onModelSelect) onModelSelect();
    };

    return (
        <div>
            <div className="space-y-4 pb-6">
                <div>
                    <Label className="block text-sm font-medium text-gray-700 mb-1">
                        전사 엔진
                    </Label>
                    <div className="mx-1 rounded-md border border-blue-200 bg-blue-50 px-3 py-2 text-sm text-blue-800 dark:border-blue-800 dark:bg-blue-950/40 dark:text-blue-200">
                        Cohere Transcribe 03-2026 (로컬 ONNX) — 한국어 기본
                    </div>
                </div>

                <div className="mt-4">
                    <CohereModelManager
                        selectedModel={transcriptModelConfig.model}
                        onModelSelect={handleCohereModelSelect}
                        autoSave={true}
                    />
                </div>
            </div>
        </div>
    );
}
