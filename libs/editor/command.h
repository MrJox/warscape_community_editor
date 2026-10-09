#pragma once
// Undo/redo. Every edit a document accepts is a Command pushed on the document's CommandStack: apply() does it, revert() takes it back.
// Drag gestures (a gizmo drag, a slider drag) push many small commands with the same non-zero merge key; the stack folds them into one
// entry until break_merge() is called (the UI calls it when the gesture ends). The stack also tells whether the document differs from its
// last saved state. Documents wrap their own lossless models (XmlDoc, TedScene, ...) in commands; the stack knows nothing about them.
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class Command {
public:
    virtual ~Command() = default;
    virtual std::string name() const = 0;
    virtual void apply() = 0;
    virtual void revert() = 0;
    // Non-zero: commands with equal keys that follow each other may merge (merge_from keeps this command's `before` and takes `next`'s `after`).
    virtual uint64_t merge_key() const { return 0; }
    virtual bool merge_from(const Command& next) { (void)next; return false; }
    // Items (Document-defined ids) this command touched, so that only they are re-published. Empty = everything.
    virtual std::vector<uint64_t> dirty_items() const { return {}; }
};

class CommandStack {
public:
    // Applies the command and records it. Drops the redo tail.
    void push(std::unique_ptr<Command> c);
    bool undo();
    bool redo();
    bool can_undo() const { return position_ > 0; }
    bool can_redo() const { return position_ < entries_.size(); }
    void break_merge() { merge_open_ = false; }

    // The history, oldest first; entries [0, position) are applied.
    size_t size() const { return entries_.size(); }
    size_t position() const { return position_; }
    std::string name(size_t i) const { return entries_[i]->name(); }
    // Undo or redo until `position` entries are applied.
    void jump_to(size_t position);

    // Saved state tracking: modified() is false right after mark_saved() and again after undoing back to that point.
    void mark_saved() { saved_position_ = position_; saved_valid_ = true; }
    bool modified() const { return saved_valid_ ? position_ != saved_position_ : position_ != 0; }
    // Changes whenever an entry is pushed, merged, undone or redone (documents re-publish when it moves).
    uint64_t version() const { return version_; }
    // Items touched by the most recent push/undo/redo ({} with `all` set when unknown).
    struct Dirty {
        bool all = true;
        std::vector<uint64_t> items;
    };
    const Dirty& last_dirty() const { return last_dirty_; }
    void clear();

private:
    void note(const Command& c) {
        last_dirty_.items = c.dirty_items();
        last_dirty_.all = last_dirty_.items.empty();
        ++version_;
    }

    std::vector<std::unique_ptr<Command>> entries_;
    size_t position_ = 0;
    size_t saved_position_ = 0;
    bool saved_valid_ = false;
    bool merge_open_ = false;
    uint64_t version_ = 0;
    Dirty last_dirty_;
};

} // namespace wce
