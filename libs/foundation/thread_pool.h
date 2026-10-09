#pragma once
// Worker threads. Two flavours on one class:
//  - parallel_for(n, grain, f): splits [0, n) into chunks that the workers and the calling thread claim from an atomic counter; returns when all
//    are done. f(begin, end, worker) with worker 0 = the caller, 1.. = pool threads (index scratch buffers with it). Safe to nest and to call from
//    a pool thread: the caller always takes part, so it cannot wait on a pool that is busy.
//  - post(f): a background job (file reading, decoding, GPU resource creation), run in priority order (smaller first); the priority of a queued job
//    can be changed with the handle `post` returns (the renderer re-ranks pending model loads by distance every frame).
// compute_pool() has one thread per core less one, io_pool() a handful of threads for jobs that wait on the disk.
#include <atomic>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <functional>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

namespace wce {

class ThreadPool {
public:
    explicit ThreadPool(int threads);
    ~ThreadPool();
    ThreadPool(const ThreadPool&) = delete;
    ThreadPool& operator=(const ThreadPool&) = delete;

    int threads() const { return int(workers_.size()); }
    int slots() const { return threads() + 1; }       // distinct `worker` values parallel_for passes

    // f(size_t begin, size_t end, int worker)
    template <class F>
    void parallel_for(size_t n, size_t grain, F&& f) {
        if (n == 0) return;
        if (grain == 0) grain = 1;
        if (threads() == 0 || n <= grain) {
            f(size_t(0), n, 0);
            return;
        }
        run_parallel(n, grain, std::function<void(size_t, size_t, int)>(std::forward<F>(f)));
    }

    using Job = std::function<void()>;
    struct Handle {
        uint64_t id = 0;
    };
    Handle post(Job job, float priority = 0.0f);
    void set_priority(Handle h, float priority);       // no-op once the job has started
    bool cancel(Handle h);                              // removes a job that has not started; false if it already did
    static float current_priority();                   // of the job the calling pool thread is running (0 elsewhere)
    size_t queued() const;
    size_t running() const { return running_.load(); }
    // Blocks until the queue is empty and no job runs.
    void wait_idle();

private:
    struct Task {
        uint64_t id;
        float priority;
        Job job;
    };
    struct Parallel;
    void run_parallel(size_t n, size_t grain, std::function<void(size_t, size_t, int)> f);
    void worker_main(int index);

    std::vector<std::thread> workers_;
    mutable std::mutex mutex_;
    std::condition_variable cv_, idle_cv_;
    std::vector<Task> queue_;                           // small (hundreds): scanned for the best priority
    std::vector<std::shared_ptr<Parallel>> parallel_;   // parallel_for jobs with chunks left
    uint64_t next_id_ = 1;
    std::atomic<size_t> running_{0};
    bool stop_ = false;
};

ThreadPool& compute_pool();   // parallel_for
ThreadPool& io_pool();        // post(): loading

} // namespace wce
