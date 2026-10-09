#include "editor/command.h"

namespace wce {

void CommandStack::push(std::unique_ptr<Command> c) {
    if (position_ < entries_.size()) {   // the redo tail goes; the saved state may have been in it
        if (saved_valid_ && saved_position_ > position_) saved_valid_ = false;
        entries_.resize(position_);
        merge_open_ = false;
    }
    c->apply();
    if (merge_open_ && position_ > 0 && c->merge_key() != 0 && entries_[position_ - 1]->merge_key() == c->merge_key() &&
        !(saved_valid_ && saved_position_ == position_) && entries_[position_ - 1]->merge_from(*c)) {
        note(*entries_[position_ - 1]);
        return;
    }
    note(*c);
    entries_.push_back(std::move(c));
    position_ = entries_.size();
    merge_open_ = entries_.back()->merge_key() != 0;
}

bool CommandStack::undo() {
    if (!can_undo()) return false;
    merge_open_ = false;
    entries_[position_ - 1]->revert();
    note(*entries_[position_ - 1]);
    --position_;
    return true;
}

bool CommandStack::redo() {
    if (!can_redo()) return false;
    merge_open_ = false;
    entries_[position_]->apply();
    note(*entries_[position_]);
    ++position_;
    return true;
}

void CommandStack::jump_to(size_t position) {
    if (position > entries_.size()) position = entries_.size();
    while (position_ > position) undo();
    while (position_ < position) redo();
}

void CommandStack::clear() {
    entries_.clear();
    position_ = saved_position_ = 0;
    saved_valid_ = false;
    merge_open_ = false;
    last_dirty_ = Dirty{};
    ++version_;
}

} // namespace wce
