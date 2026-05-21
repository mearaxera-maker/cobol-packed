# Third-Party Notices

## Unicode ICU Mapping Data

`src/cli/ebcdic_tables.rs` and `src/cli/mixed_dbcs_tables.rs` contain generated
Unicode mappings derived from Unicode ICU mapping data and local platform
codepage tables.

The mixed DBCS tables were generated from these Unicode ICU `.ucm` files:

- `ibm-930_P120-1999.ucm`
- `ibm-933_P110-1995.ucm`
- `ibm-935_P110-1999.ucm`
- `ibm-937_P110-1999.ucm`
- `ibm-939_P120-1999.ucm`

Source repository:
<https://github.com/unicode-org/icu/tree/main/icu4c/source/data/mappings>

Unicode license and terms:
<https://www.unicode.org/copyright.html>
