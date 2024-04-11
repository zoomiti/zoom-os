use core::task::{Context, Poll, Waker};

use alloc::{collections::BTreeMap, sync::Arc, task::Wake};
use crossbeam_queue::SegQueue;
use tracing::debug;
use x86_64::instructions::interrupts;

use crate::util::r#async::mutex::Mutex;

use super::{Task, TaskId};

static EXECUTOR: Executor = Executor::new();

pub struct Executor {
    task_queue: SegQueue<TaskId>,
    spawn_queue: SegQueue<Task>,
    task_waker_list: Mutex<BTreeMap<TaskId, (Task, Waker)>>,
}

pub fn spawn(task: impl Into<Task>) {
    let task = task.into();
    EXECUTOR.spawn_queue.push(task);
}

pub fn run() -> ! {
    loop {
        EXECUTOR.run_ready_tasks();
        EXECUTOR.sleep_if_idle();
    }
}

impl Executor {
    pub const fn new() -> Self {
        Self {
            task_queue: SegQueue::new(),
            spawn_queue: SegQueue::new(),
            task_waker_list: Mutex::new(BTreeMap::new()),
        }
    }

    fn run_ready_tasks(&'static self) {
        let Self {
            task_queue,
            spawn_queue,
            task_waker_list,
        } = self;

        // get the spawn queue
        {
            let mut task_waker = task_waker_list.spin_lock();
            while let Some(task) = spawn_queue.pop() {
                let id = task.id;
                task_waker.insert(task.id, (task, TaskWaker::new(id, task_queue).into()));
                task_queue.push(id);
            }
        }

        while let Some(task_id) = task_queue.pop() {
            let mut task_waker = task_waker_list.spin_lock();
            let Some((task, waker)) = task_waker.get_mut(&task_id) else {
                debug!(task_id = task_id.0, "Task was woken up more than necessary");
                continue;
            };

            let mut context = Context::from_waker(waker);

            match task.poll(&mut context) {
                Poll::Ready(()) => {
                    task_waker.remove(&task_id);
                }
                Poll::Pending => {
                    //self.task_waker.spin_lock().insert(task_id, (task, waker));
                }
            }
        }
    }

    fn sleep_if_idle(&self) {
        interrupts::disable();
        if self.task_queue.is_empty() {
            interrupts::enable_and_hlt();
        } else {
            interrupts::enable();
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

struct TaskWaker {
    task_id: TaskId,
    task_queue: &'static SegQueue<TaskId>,
}

impl TaskWaker {
    fn new(task_id: TaskId, task_queue: &'static SegQueue<TaskId>) -> TaskWaker {
        Self {
            task_id,
            task_queue,
        }
    }

    fn wake_task(&self) {
        self.task_queue.push(self.task_id);
    }
}

impl From<TaskWaker> for Waker {
    fn from(value: TaskWaker) -> Self {
        Self::from(Arc::new(value))
    }
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_task();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wake_task();
    }
}
