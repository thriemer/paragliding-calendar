# Calendar Integration

What do I want?

- For now only Google Calendar
- Scheduled to run once a day and then exit -> not continuous running
- Add time entries, where a flying site is flyable along with avg flying score
- Add buffer for driving from/to paragliding site
    - Assume that I will be driving from home
- Look up my schedule and don't put flying events when other events are planned

How do I implement this?

1. Load all calendar events.
2. Remove all flyable time slots that overlap with calendar events.
3. Merge the flyable time slots to time ranges.
4. Subtract driving times from time ranges.
5. Filter again: no time left in range, driving time longer than potential flying time

What about multiple flying sites?

- Try to show all of them, add additional filters if it gets too annoying

How to manage existing events?

- Clear all existing entries and recreate them -> way easier than to match and edit
