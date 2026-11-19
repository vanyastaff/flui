### Fixed

- **`flui-widgets`**: `Table` lays its columns out from the start edge of the ambient
  `Directionality`, so column 0 sits on the right in RTL, and a direction change relays the
  table out in place.
- **`flui-material`**: `DataTable` follows the ambient `Directionality`: in RTL the checkbox
  column leads on the right, cell padding names the start and end edges, and a non-numeric cell
  aligns to the start edge and a numeric one to the end edge.
