'use client';
import { uiText, useUiTranslation } from '@/i18n/ui';

interface ConfidenceIndicatorProps {
  confidence: number;
  showIndicator?: boolean;
}

export const ConfidenceIndicator: React.FC<ConfidenceIndicatorProps> = ({
  confidence,
  showIndicator = true,
}) => {
  useUiTranslation();
  // Don't render if preference is disabled
  if (!showIndicator) {
    return null;
  }

  // Get color class based on confidence threshold
  const getColorClass = (conf: number): string => {
    if (conf >= 0.8) return 'bg-green-500'; // 80-100%: High confidence
    if (conf >= 0.7) return 'bg-yellow-500'; // 70-79%: Good confidence
    if (conf >= 0.4) return 'bg-orange-500'; // 40-79%: Medium confidence
    return 'bg-red-500'; // Below 50%: Low confidence
  };

  // Get descriptive label for accessibility
  const getConfidenceLabel = (conf: number): string => {
    if (conf >= 0.8) return uiText("messages.highConfidence");
    if (conf >= 0.7) return uiText("messages.goodConfidence");
    if (conf >= 0.4) return uiText("messages.mediumConfidence");
    return uiText("messages.lowConfidence");
  };

  const confidencePercent = (confidence * 100).toFixed(0);
  const colorClass = getColorClass(confidence);
  const label = getConfidenceLabel(confidence);

  return (
    <div
      className="flex items-center gap-1"
      title={uiText("messages.confidence", { value0: confidencePercent, value1: label })}
      aria-label={uiText("messages.transcriptionConfidence", { value0: confidencePercent })}
    >
      <div
        className={`w-2 h-2 rounded-full ${colorClass} transition-colors duration-200`}
        role="status"
      />
    </div>
  );
};
