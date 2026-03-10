import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ModelConfig, ModelSettingsModal } from "./ModelSettingsModal"
import { TranscriptModelProps, TranscriptSettings } from "./TranscriptSettings"
import { RecordingSettings, RecordingPreferences } from "./RecordingSettings"
import { PersonalKeysSettings } from "./PersonalKeysSettings";

interface SettingTabsProps {
  modelConfig: ModelConfig;
  setModelConfig: (config: ModelConfig | ((prev: ModelConfig) => ModelConfig)) => void;
  onSave: (config: ModelConfig) => void;
  transcriptModelConfig: TranscriptModelProps;
  setTranscriptModelConfig: (config: TranscriptModelProps) => void;
  onSaveTranscript: (config: TranscriptModelProps) => void;
  setSaveSuccess: (success: boolean | null) => void;
  defaultTab?: string;
}

export function SettingTabs({
  modelConfig,
  setModelConfig,
  onSave,
  setSaveSuccess,
  defaultTab = "personalKeys",
  transcriptModelConfig,
  setTranscriptModelConfig,
  onSaveTranscript,
}: SettingTabsProps) {

  const handleTabChange = () => {
    setSaveSuccess(null); // Reset save success when tab changes
  };

  return (
    <Tabs defaultValue={defaultTab} className="w-full max-h-[calc(100vh-10rem)] overflow-y-auto" onValueChange={handleTabChange}>
      <TabsList>
        {/* <TabsTrigger value="transcriptSettings">Transcript</TabsTrigger> */}
        {/* <TabsTrigger value="modelSettings">Ai Summary</TabsTrigger> */}
        <TabsTrigger value="personalKeys">Personal Keys</TabsTrigger>
        {/* <TabsTrigger value="recordingSettings">Preferences</TabsTrigger> */}
      </TabsList>
      {/* <TabsContent value="modelSettings">
        <ModelSettingsModal

          modelConfig={modelConfig}
          setModelConfig={setModelConfig}
          onSave={onSave}
        />
      </TabsContent> */}
      <TabsContent value="personalKeys">
        <PersonalKeysSettings />
      </TabsContent>
      {/* <TabsContent value="transcriptSettings">
        <TranscriptSettings
          transcriptModelConfig={transcriptModelConfig}
          setTranscriptModelConfig={setTranscriptModelConfig}
        // onSave={onSaveTranscript}
        />
      </TabsContent> */}
      {/* <TabsContent value="recordingSettings">
        <RecordingSettings />
      </TabsContent> */}
    </Tabs>
  )
}


