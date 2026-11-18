### Fixed

- A window frame that the GPU's submission ledger refuses, because the GPU retires no work, no longer uploads its texture images. Before, each refused frame of a streaming texture held another batch of staging memory that could not be flushed. Window slots now refuse such a frame before they write, as offscreen slots already did.
