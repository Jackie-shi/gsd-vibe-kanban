/**
 * Task Priority Module — test feature for OpenClaw workflow validation
 * Jira Ticket: SMP-4 (示例任务 2)
 */

const PRIORITY_LEVELS = {
  HIGH: { value: 3, label: 'High', color: '#ef4444' },
  MEDIUM: { value: 2, label: 'Medium', color: '#f59e0b' },
  LOW: { value: 1, label: 'Low', color: '#22c55e' },
};

function assignPriority(task, level) {
  if (!PRIORITY_LEVELS[level]) {
    throw new Error(`Invalid priority: ${level}. Use HIGH, MEDIUM, or LOW.`);
  }
  return { ...task, priority: PRIORITY_LEVELS[level] };
}

function sortByPriority(tasks) {
  return [...tasks].sort((a, b) => (b.priority?.value || 0) - (a.priority?.value || 0));
}

// Demo
const tasks = [
  assignPriority({ id: 'SMP-4', title: '示例任务 2' }, 'HIGH'),
  assignPriority({ id: 'SMP-5', title: '示例任务 3' }, 'LOW'),
  assignPriority({ id: 'SMP-6', title: '示例任务 4' }, 'MEDIUM'),
];

console.log('Sorted by priority:');
sortByPriority(tasks).forEach(t =>
  console.log(`  [${t.priority.label}] ${t.id}: ${t.title}`)
);

module.exports = { PRIORITY_LEVELS, assignPriority, sortByPriority };
