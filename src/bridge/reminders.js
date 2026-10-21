// today — Apple Reminders bridge (JXA). Run by `osascript -l JavaScript`.
// Prints a JSON array of open reminders that have a due date on or before
// today. Arguments (optional) are list names to include.
// Read-only: it never modifies a reminder.
function run(argv) {
  const app = Application('Reminders');
  const wanted = argv && argv.length ? argv : null;
  const end = new Date();
  end.setHours(23, 59, 59, 999);
  const pad = (n) => (n < 10 ? '0' : '') + n;
  const ymd = (d) => d.getFullYear() + '-' + pad(d.getMonth() + 1) + '-' + pad(d.getDate());
  const out = [];
  app.lists().forEach((list) => {
    const listName = list.name();
    if (wanted && wanted.indexOf(listName) < 0) return;
    const open = list.reminders.whose({ completed: false });
    // Bulk property reads: one Apple Event per property, not per reminder.
    const names = open.name();
    const dues = open.dueDate();
    const prios = open.priority();
    for (let i = 0; i < names.length; i++) {
      const due = dues[i];
      if (!due || due > end) continue;
      out.push({ title: names[i], list: listName, due: ymd(due), priority: prios[i] || 0 });
    }
  });
  return JSON.stringify(out);
}
