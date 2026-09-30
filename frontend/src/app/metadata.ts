import { uiText } from '@/i18n/ui';
import { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Meetily',
  get description() { return uiText("messages.aIPoweredMeetingAssistant"); },
}
