import ICAL from 'ical.js';

export interface CalendarMeeting {
  id: string;
  title: string;
  start: Date;
  end: Date;
  joinUrl: string | null;
}

export interface CalendarAutomationSettings {
  enabled: boolean;
  icalUrl: string;
  conferenceOnly: boolean;
  autoStop: boolean;
}

export const DEFAULT_CALENDAR_AUTOMATION_SETTINGS: CalendarAutomationSettings = {
  enabled: false,
  icalUrl: '',
  conferenceOnly: true,
  autoStop: true,
};

const CONFERENCE_URL_PATTERN = /https?:\/\/(?:meet\.google\.com|(?:[a-z0-9-]+\.)?zoom\.us|teams\.microsoft\.com|teams\.live\.com|(?:[a-z0-9-]+\.)?webex\.com|whereby\.com|app\.slack\.com|discord\.com)\/[^\s<>"']+/i;
const MAX_OCCURRENCES_PER_EVENT = 10_000;

type IcalEvent = InstanceType<typeof ICAL.Event>;

function normalizedPropertyText(event: IcalEvent): string {
  const propertyValues = event.component
    .getAllProperties()
    .flatMap((property) => property.getValues())
    .map((value) => String(value));

  return [event.summary, event.description, event.location, ...propertyValues]
    .filter(Boolean)
    .join('\n')
    .replaceAll('\\/', '/')
    .replaceAll('&amp;', '&');
}

function extractConferenceUrl(event: IcalEvent): string | null {
  const match = normalizedPropertyText(event).match(CONFERENCE_URL_PATTERN);
  return match?.[0]?.replace(/[),.;]+$/, '') ?? null;
}

function isEligibleEvent(event: IcalEvent): boolean {
  const status = String(event.component.getFirstPropertyValue('status') ?? '').toUpperCase();
  const transparency = String(event.component.getFirstPropertyValue('transp') ?? '').toUpperCase();
  return status !== 'CANCELLED' && transparency !== 'TRANSPARENT';
}

function addOccurrence(
  meetings: CalendarMeeting[],
  event: IcalEvent,
  start: Date,
  end: Date,
  rangeStart: Date,
  rangeEnd: Date,
) {
  if (event.startDate.isDate || end <= rangeStart || start >= rangeEnd || end <= start) {
    return;
  }

  meetings.push({
    id: `${event.uid || 'event'}:${start.toISOString()}`,
    title: event.summary?.trim() || 'Calendar meeting',
    start,
    end,
    joinUrl: extractConferenceUrl(event),
  });
}

export function parseGoogleCalendarIcs(
  icsText: string,
  rangeStart: Date,
  rangeEnd: Date,
): CalendarMeeting[] {
  if (rangeEnd <= rangeStart) {
    throw new Error('Calendar range end must be after its start.');
  }

  const calendar = new ICAL.Component(ICAL.parse(icsText));
  for (const timezone of calendar.getAllSubcomponents('vtimezone')) {
    ICAL.TimezoneService.register(timezone);
  }
  const components = calendar.getAllSubcomponents('vevent');
  const meetings: CalendarMeeting[] = [];

  for (const component of components) {
    const event = new ICAL.Event(component);
    if (event.isRecurrenceException() || !isEligibleEvent(event)) {
      continue;
    }

    if (!event.isRecurring()) {
      addOccurrence(
        meetings,
        event,
        event.startDate.toJSDate(),
        event.endDate.toJSDate(),
        rangeStart,
        rangeEnd,
      );
      continue;
    }

    const iterator = event.iterator();
    let occurrences = 0;
    let occurrence = iterator.next();
    while (occurrence && occurrences < MAX_OCCURRENCES_PER_EVENT) {
      occurrences += 1;
      const details = event.getOccurrenceDetails(occurrence);
      const start = details.startDate.toJSDate();
      const end = details.endDate.toJSDate();

      if (start >= rangeEnd) {
        break;
      }

      if (end > rangeStart && isEligibleEvent(details.item)) {
        addOccurrence(meetings, details.item, start, end, rangeStart, rangeEnd);
      }

      occurrence = iterator.next();
    }
  }

  const uniqueMeetings = new Map(meetings.map((meeting) => [meeting.id, meeting]));
  return [...uniqueMeetings.values()].sort(
    (left, right) => left.start.getTime() - right.start.getTime() || left.id.localeCompare(right.id),
  );
}

export function filterCalendarMeetings(
  meetings: CalendarMeeting[],
  conferenceOnly: boolean,
): CalendarMeeting[] {
  return conferenceOnly ? meetings.filter((meeting) => meeting.joinUrl !== null) : meetings;
}

export function selectActiveCalendarMeeting(
  meetings: CalendarMeeting[],
  now: Date,
): CalendarMeeting | null {
  const timestamp = now.getTime();
  return meetings.find(
    (meeting) => meeting.start.getTime() <= timestamp && timestamp < meeting.end.getTime(),
  ) ?? null;
}

export function selectNextCalendarMeeting(
  meetings: CalendarMeeting[],
  now: Date,
): CalendarMeeting | null {
  const timestamp = now.getTime();
  return meetings.find((meeting) => meeting.start.getTime() > timestamp) ?? null;
}

export function isGoogleCalendarIcsUrl(value: string): boolean {
  try {
    const url = new URL(value.trim());
    const path = url.pathname.toLowerCase();
    return url.protocol === 'https:'
      && ['calendar.google.com', 'calendar.googleusercontent.com'].includes(url.hostname)
      && path.includes('/calendar/ical/')
      && (path.endsWith('/basic.ics') || path.endsWith('/full.ics'));
  } catch {
    return false;
  }
}
