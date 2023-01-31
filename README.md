# Entremets

An *entremets* is a delicious dessert. It literally means between meals.

Entremets on the other hand is a testing library to intertwine your sql queries.

## Goal

It's difficult to identify serializable anomalies.
This project aims at:

- Provide a simple language to translate production code
- Quickly find anomalies
- Test solutions

## Isolation Level is a mess

Sql engines offer different level of isolation.
They can be qualified as the following:

| Isolation        | Quality    |
|------------------|------------|
| Read Uncommitted | Shit       |
| Read Committed   | Less Shit  |
| Repeatable Read  | Still Shit |
| Serializable     | Shit [^1]  |

[^1]: Serializable is the intuitive isolation you'd expect from a transaction. But it's performance makes it shit.

Most application uses _Read Committed_ isolation.

## Lost update scenario

Take this scenario where the two processes run concurrently.
The code in entremets is:

``` mets
init do
    insert into users (id, age) values (1, 10)
end

process do
    begin read_committed
    let t1_age := select age from users where id = 1
    update users set age := t1_age + 1 where id = 1
    commit
end

process do
    begin read_committed
    let t2_age := select age from users where id = 1
    update users set age := t2_age * 2 where id = 1
    commit
end
```

You'd expect `select age from users where id = 1` to be either `21` or `22` when both process are completed.

In entremets, you can test this property by adding this line to the specifications:

``` mets
property = eventually<select age from users where id = 1 in {21, 22}>
```

Now lets run this mets (mets are the specification, entremets finds the anomalies) under entremets:

```
> entremets lost_update.mets
Following property was violated: eventually<select age from users where id = 1 in {21, 22}>
The following counter example was found:
...
```

The output shows a counter example for the property.
It shows the trace of what happened to obtain this state.

## State of entremets

For now entremets is in very early stage.
It only supports read committed isolation level and very narrow sql syntax.

Do not hesitate to file issues for any features you feel is missing.