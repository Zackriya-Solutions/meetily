import { describe, expect, test } from 'bun:test';
import {
  filterCalendarMeetings,
  isGoogleCalendarIcsUrl,
  parseGoogleCalendarIcs,
  selectActiveCalendarMeeting,
  selectNextCalendarMeeting,
} from '../../src/lib/calendar-automation';

const calendar = (...events: string[]) => [
  'BEGIN:VCALENDAR',
  'VERSION:2.0',
  'PRODID:-//Meetily Tests//EN',
  ...events,
  'END:VCALENDAR',
].join('\r\n');

const event = (lines: string[]) => ['BEGIN:VEVENT', ...lines, 'END:VEVENT'].join('\r\n');

describe('Google Calendar automation', () => {
  test('parses timed meetings and extracts supported conference links', () => {
    const meetings = parseGoogleCalendarIcs(
      calendar(event([
        'UID:planning@example.com',
        'DTSTART:20260911T100000Z',
        'DTEND:20260911T110000Z',
        'SUMMARY:Product planning',
        'DESCRIPTION:Join https://meet.google.com/abc-defg-hij',
      ])),
      new Date('2026-09-11T00:00:00Z'),
      new Date('2026-09-12T00:00:00Z'),
    );

    expect(meetings).toHaveLength(1);
    expect(meetings[0]).toMatchObject({
      title: 'Product planning',
      joinUrl: 'https://meet.google.com/abc-defg-hij',
    });
  });

  test('expands recurring events and applies recurrence exceptions', () => {
    const meetings = parseGoogleCalendarIcs(
      calendar(
        event([
          'UID:standup@example.com',
          'DTSTART:20260907T090000Z',
          'DTEND:20260907T093000Z',
          'RRULE:FREQ=DAILY;COUNT=5',
          'SUMMARY:Daily standup',
          'LOCATION:https://zoom.us/j/123456789',
        ]),
        event([
          'UID:standup@example.com',
          'RECURRENCE-ID:20260909T090000Z',
          'DTSTART:20260909T100000Z',
          'DTEND:20260909T103000Z',
          'SUMMARY:Delayed standup',
          'LOCATION:https://zoom.us/j/123456789',
        ]),
      ),
      new Date('2026-09-08T00:00:00Z'),
      new Date('2026-09-12T00:00:00Z'),
    );

    expect(meetings.map((meeting) => meeting.start.toISOString())).toEqual([
      '2026-09-08T09:00:00.000Z',
      '2026-09-09T10:00:00.000Z',
      '2026-09-10T09:00:00.000Z',
      '2026-09-11T09:00:00.000Z',
    ]);
    expect(meetings[1].title).toBe('Delayed standup');
  });

  test('ignores all-day, cancelled, and transparent events', () => {
    const meetings = parseGoogleCalendarIcs(
      calendar(
        event([
          'UID:holiday@example.com',
          'DTSTART;VALUE=DATE:20260911',
          'DTEND;VALUE=DATE:20260912',
          'SUMMARY:Holiday',
        ]),
        event([
          'UID:cancelled@example.com',
          'DTSTART:20260911T100000Z',
          'DTEND:20260911T110000Z',
          'STATUS:CANCELLED',
          'SUMMARY:Cancelled',
        ]),
        event([
          'UID:focus@example.com',
          'DTSTART:20260911T120000Z',
          'DTEND:20260911T130000Z',
          'TRANSP:TRANSPARENT',
          'SUMMARY:FYI',
        ]),
      ),
      new Date('2026-09-11T00:00:00Z'),
      new Date('2026-09-12T00:00:00Z'),
    );

    expect(meetings).toEqual([]);
  });

  test('uses timezone definitions embedded in the calendar feed', () => {
    const meetings = parseGoogleCalendarIcs(`BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VTIMEZONE
TZID:Test/Offset
BEGIN:STANDARD
DTSTART:19700101T000000
TZOFFSETFROM:+0530
TZOFFSETTO:+0530
TZNAME:IST
END:STANDARD
END:VTIMEZONE
BEGIN:VEVENT
UID:timezone-meeting
DTSTART;TZID=Test/Offset:20260911T100000
DTEND;TZID=Test/Offset:20260911T110000
SUMMARY:Cross-timezone meeting
LOCATION:https://meet.google.com/abc-defg-hij
END:VEVENT
END:VCALENDAR`, new Date('2026-09-11T00:00:00.000Z'), new Date('2026-09-12T00:00:00.000Z'));

    expect(meetings).toHaveLength(1);
    expect(meetings[0].start.toISOString()).toBe('2026-09-11T04:30:00.000Z');
    expect(meetings[0].end.toISOString()).toBe('2026-09-11T05:30:00.000Z');
  });

  test('filters non-conference events and selects active and next meetings', () => {
    const meetings = parseGoogleCalendarIcs(
      calendar(
        event([
          'UID:call@example.com',
          'DTSTART:20260911T100000Z',
          'DTEND:20260911T110000Z',
          'SUMMARY:Customer call',
          'URL:https://teams.microsoft.com/l/meetup-join/abc',
        ]),
        event([
          'UID:focus@example.com',
          'DTSTART:20260911T120000Z',
          'DTEND:20260911T130000Z',
          'SUMMARY:Focus block',
        ]),
      ),
      new Date('2026-09-11T00:00:00Z'),
      new Date('2026-09-12T00:00:00Z'),
    );

    expect(filterCalendarMeetings(meetings, true).map((meeting) => meeting.title)).toEqual([
      'Customer call',
    ]);
    expect(selectActiveCalendarMeeting(meetings, new Date('2026-09-11T10:30:00Z'))?.title)
      .toBe('Customer call');
    expect(selectNextCalendarMeeting(meetings, new Date('2026-09-11T10:30:00Z'))?.title)
      .toBe('Focus block');
  });

  test('only accepts Google-hosted HTTPS iCal feed URLs', () => {
    expect(isGoogleCalendarIcsUrl(
      'https://calendar.google.com/calendar/ical/user%40example.com/private-token/basic.ics',
    )).toBe(true);
    expect(isGoogleCalendarIcsUrl('https://example.com/calendar/ical/a/basic.ics')).toBe(false);
    expect(isGoogleCalendarIcsUrl(
      'http://calendar.google.com/calendar/ical/user%40example.com/private/basic.ics',
    )).toBe(false);
  });
});
