#pragma once
// The weather selection database (warscape_asset_variation_db): which .environment file the game picks for a set of battle conditions,
// and an editor for its value lists and variations (see docs/re/variation_db.md). Loads the .bin the game uses from the packs (or any .bin /
// .xml), edits in memory, saves XML and FASTBIN as loose files under the output folder.
#include "formats/variation_db.h"
#include "weather/variation_editor.h"

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace wce {

class Editor;

class WeatherPicker {
public:
    bool open = false;                 // the "Weather picker" window is shown
    // Draws the window; `open_file` opens an in-pack path in the editor, `active_environment` is the in-pack path of the open environment (reverse lookup).
    void draw(Editor& editor, const std::filesystem::path& output_dir, const std::string& active_environment);

    // Test and command-line hooks
    void set_script(const std::string& s) { script_ = s; }                                  // run once after the database loaded
    void set_pick(const std::string& category, const std::string& value) { overrides_[category] = value; if (db_) query_[category] = value; }
    void show_edit_tab() { select_pick_ = false; }
    const std::string& message() const { return message_; }
    // Loads the database now (otherwise on first draw) and runs a pending script; true when a database is loaded.
    bool ensure_loaded(Editor& editor, const std::filesystem::path& output_dir);
    const VariationDb* db() const { return db_.get(); }
    // The environment files the game would choose for the current query: {path, allowed values}.
    VariationDb::Result candidates() const { return db_ ? db_->candidates("environment", query_) : VariationDb::Result{}; }

private:
    void load(Editor& editor);
    void load_source(Editor& editor, int index);
    void load_file(const std::string& path);
    void run_script(const std::filesystem::path& output_dir);
    void save(const std::filesystem::path& output_dir, bool xml, bool bin);

    std::unique_ptr<VariationDb> db_;
    VariationDb orig_;                      // as loaded, to tell whether the working copy was edited
    std::string error_, source_, message_;
    struct Source { std::string label; int pack; bool xml; };
    std::vector<Source> sources_;
    int source_index_ = 0;
    char loose_[512] = {};
    bool save_anyway_ = false;
    VariationEditState edit_;
    std::string script_;
    bool select_pick_ = true;               // the Pick tab is shown first
    bool loaded_ = false;
    VariationDb::Query query_, overrides_;  // overrides: command line / command, applied when the database loads
};

} // namespace wce
