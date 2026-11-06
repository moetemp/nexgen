A Python `PollIterator` has `resume_request`, which is `next_request` but stays set after the last page, so a read that ended can resume where it stopped.
