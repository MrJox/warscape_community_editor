#include "foundation/thread_pool.h"

#include <algorithm>

namespace wce {

namespace {
thread_local float t_priority = 0.0f;
}

float ThreadPool::current_priority() { return t_priority; }

struct ThreadPool::Parallel {
    size_t n = 0, grain = 1, chunks = 0;
    std::function<void(size_t, size_t, int)> f;
    std::atomic<size_t> next{0};
    std::atomic<size_t> done{0};
};

ThreadPool::ThreadPool(int threads) {
    for (int i = 0; i < threads; ++i) workers_.emplace_back([this, i] { worker_main(i + 1); });
}

ThreadPool::~ThreadPool() {
    {
        std::lock_guard<std::mutex> lock(mutex_);
        stop_ = true;
    }
    cv_.notify_all();
    for (auto& t : workers_) t.join();
}

void ThreadPool::run_parallel(size_t n, size_t grain, std::function<void(size_t, size_t, int)> f) {
    auto p = std::make_shared<Parallel>();
    p->n = n;
    p->grain = grain;
    p->chunks = (n + grain - 1) / grain;
    p->f = std::move(f);
    {
        std::lock_guard<std::mutex> lock(mutex_);
        parallel_.push_back(p);
    }
    cv_.notify_all();
    for (;;) {
        const size_t c = p->next.fetch_add(1, std::memory_order_relaxed);
        if (c >= p->chunks) break;
        const size_t b = c * p->grain;
        p->f(b, std::min(p->n, b + p->grain), 0);
        p->done.fetch_add(1, std::memory_order_release);
    }
    // all chunks are claimed; wait for the ones other threads are still running
    while (p->done.load(std::memory_order_acquire) < p->chunks) std::this_thread::yield();
    std::lock_guard<std::mutex> lock(mutex_);
    parallel_.erase(std::remove(parallel_.begin(), parallel_.end(), p), parallel_.end());
}

void ThreadPool::worker_main(int index) {
    for (;;) {
        std::shared_ptr<Parallel> par;
        Task task;
        bool have_task = false;
        {
            std::unique_lock<std::mutex> lock(mutex_);
            cv_.wait(lock, [&] {
                if (stop_) return true;
                for (auto& p : parallel_)
                    if (p->next.load(std::memory_order_relaxed) < p->chunks) return true;
                return !queue_.empty();
            });
            if (stop_) return;
            for (auto& p : parallel_)
                if (p->next.load(std::memory_order_relaxed) < p->chunks) { par = p; break; }
            if (!par && !queue_.empty()) {
                auto best = std::min_element(queue_.begin(), queue_.end(), [](const Task& a, const Task& b) { return a.priority < b.priority; });
                task = std::move(*best);
                queue_.erase(best);
                have_task = true;
                running_.fetch_add(1);
            }
        }
        if (par) {
            for (;;) {
                const size_t c = par->next.fetch_add(1, std::memory_order_relaxed);
                if (c >= par->chunks) break;
                const size_t b = c * par->grain;
                par->f(b, std::min(par->n, b + par->grain), index);
                par->done.fetch_add(1, std::memory_order_release);
            }
        } else if (have_task) {
            t_priority = task.priority;
            task.job();
            t_priority = 0.0f;
            running_.fetch_sub(1);
            std::lock_guard<std::mutex> lock(mutex_);
            idle_cv_.notify_all();
        }
    }
}

ThreadPool::Handle ThreadPool::post(Job job, float priority) {
    Handle h;
    if (workers_.empty()) {   // no threads: run it now
        job();
        return h;
    }
    {
        std::lock_guard<std::mutex> lock(mutex_);
        h.id = next_id_++;
        queue_.push_back({h.id, priority, std::move(job)});
    }
    cv_.notify_one();
    return h;
}

void ThreadPool::set_priority(Handle h, float priority) {
    std::lock_guard<std::mutex> lock(mutex_);
    for (Task& t : queue_)
        if (t.id == h.id) { t.priority = priority; return; }
}

bool ThreadPool::cancel(Handle h) {
    std::lock_guard<std::mutex> lock(mutex_);
    for (auto it = queue_.begin(); it != queue_.end(); ++it)
        if (it->id == h.id) {
            queue_.erase(it);
            return true;
        }
    return false;
}

size_t ThreadPool::queued() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return queue_.size();
}

void ThreadPool::wait_idle() {
    std::unique_lock<std::mutex> lock(mutex_);
    idle_cv_.wait(lock, [&] { return queue_.empty() && running_.load() == 0; });
}

ThreadPool& compute_pool() {
    static ThreadPool pool(std::max(1, int(std::thread::hardware_concurrency()) - 1));
    return pool;
}

ThreadPool& io_pool() {
    static ThreadPool pool(std::clamp(int(std::thread::hardware_concurrency()) / 2, 2, 6));
    return pool;
}

} // namespace wce
