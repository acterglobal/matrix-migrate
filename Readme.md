# matrix migrate

CLI to migrate one matrix account to a new one. Similar to [the EMS migrator][ems tool] but:

1. is a nice little CLI tool, based on `matrix-rust-sdk`
2. allows for restarts (refreshes at the beginning)
3. it runs the operations async and is thus a lot faster
4. SSO authentication is supported

_Note_:
It currently only migrates the rooms listing and power_levels, no user settings or profile data.

## Install and use

You need a recent [Rust] installation. Then you can either clone the repository
and use `cargo run` or use cargo-install:

```
cargo install https://matrix.org/acterglobal/matrix-migrate
```

and then can run it by just doing

```
matrix-migrate
```

### Usage note

Password authentication requires both the user and password for the user `from`
and `to` either as command line parameters, or preferably as environment
variables (`FROM_USER=`, `FROM_PASSWORD`).

If no `from` and/or `to` password provided it will try login with servers's
SSO.

It uses matrix discovery but if that doesn't work for you, you can provide
custom homeservers, too.

It will start with a full-sync of the room state, so depending on the size of
your matrix account(s), this may take a moment.

## Changelog

**Unreleased**

- Add support for matching up `power_levels`, needs latest matrix-rust-sdk-git

[ems tool]: https://ems.element.io/tools/matrix-migration
