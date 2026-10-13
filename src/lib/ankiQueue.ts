type Task = { owner: object; priority: boolean; run: () => Promise<void>; cancel: () => void };
const pending: Task[] = [];
let running = false;
let currentOwner: object | null = null;

async function drain() {
  if (running) return;
  running = true;
  while (pending.length) {
    let next = pending.findIndex((task) => task.priority);
    if (next < 0) next = pending.findIndex((task) => task.owner === currentOwner);
    const [task] = pending.splice(next < 0 ? 0 : next, 1);
    await task.run();
  }
  running = false;
}

export const ankiQueue = {
  enqueue<T>(owner: object, run: () => Promise<T>, priority = false): Promise<T | null> {
    return new Promise((resolve, reject) => {
      pending.push({
        owner,
        priority,
        cancel: () => resolve(null),
        run: async () => {
          try { resolve(await run()); }
          catch (error) { reject(error); }
        },
      });
      void drain();
    });
  },
  cancelChecks(owner: object) {
    currentOwner = owner;
    for (let i = pending.length - 1; i >= 0; i--) {
      if (pending[i].owner === owner && !pending[i].priority) {
        pending.splice(i, 1)[0].cancel();
      }
    }
  },
};
