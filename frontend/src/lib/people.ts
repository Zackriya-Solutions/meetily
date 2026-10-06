import type { Person } from '@/types';

/** People whose name contains the draft, names that start with it first; at most `limit`. */
export function matchPeople(people: Person[], draft: string, limit = 5): Person[] {
  const query = draft.trim().replace(/\s+/g, ' ').toLowerCase();
  if (!query) return [];
  const prefix: Person[] = [];
  const inside: Person[] = [];
  for (const person of people) {
    const name = person.name.toLowerCase();
    if (name.startsWith(query)) prefix.push(person);
    else if (name.includes(query)) inside.push(person);
  }
  return [...prefix, ...inside].slice(0, limit);
}
