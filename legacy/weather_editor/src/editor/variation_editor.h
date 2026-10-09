#pragma once
// Editing UI for the battle weather selection database (formats/variation_db.h): value lists per category and the
// variations (environment file + the conditions it is allowed for).
#include "vfs/vfs.h"
#include "formats/variation_db.h"

#include <map>
#include <string>

namespace wce {

struct VariationEditState {
    int entry = 0;       // which entry ("environment") is shown
    int selected = -1;   // variation row being edited
    char filter[128] = {};
    std::map<std::string, std::string> new_value; // text typed into each category's "new value" box
    char new_file[300] = {};
    std::string message; // result of the last operation that can fail
};

// Draws the editor; returns true when the database changed this frame.
bool draw_variation_editor(VariationDb& db, VariationEditState& st, const Vfs* vfs);

} // namespace wce
