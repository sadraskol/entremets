Deadlocks detection
===================

Sql engine detect deadlocks and kills a transaction to break the cycle.
:code:`entremets` supports automatic deadlock detection.
You can now anticipate and avoid deadlocks in production!

.. code-block:: entremets

    init do
      `insert into users(id, age) values (1, 10), (2, 20)`
    end

    process do
      transaction tx1 read_committed do
        `update users set age := 11 where id = 1`
        `update users set age := 21 where id = 2`
      end
    end

    process do
      transaction tx2 read_committed do
        `update users set age := 22 where id = 2`
        `update users set age := 12 where id = 1`
      end
    end

Run the specification with :code:`entremets model.mets`.
The output signals for a possible deadlock scenario:

.. code-block:: text

    System ran into a deadlock:
    Process 0 holds lock on [RowId(1)] and waits for Locked(RowId(2))
    Process 1 holds lock on [RowId(2)] and waits for Locked(RowId(1))
    users: {(age: 10, id: 1), (id: 2, age: 20)}
    Process 0: begin read committed (tx1)
    Local State {"tx1": Tx(Transaction(Running))}
    users: {(age: 10, id: 1), (id: 2, age: 20)}
    Process 0: update users set age := 11 where id = 1
    Local State {"tx1": Tx(Transaction(Running))}
    users: {(age: 10, id: 1), (id: 2, age: 20)}
    Process 1: begin read committed (tx2)
    Local State {"tx1": Tx(Transaction(Running)), "tx2": Tx(Transaction(Running))}
    users: {(age: 10, id: 1), (id: 2, age: 20)}
    Process 1: update users set age := 22 where id = 2
    Local State {"tx1": Tx(Transaction(Running)), "tx2": Tx(Transaction(Running))}
    users: {(age: 10, id: 1), (id: 2, age: 20)}
    Local State {"tx1": Tx(Transaction(Running)), "tx2": Tx(Transaction(Running))}
    users: {(age: 10, id: 1), (id: 2, age: 20)}
    Local State {"tx1": Tx(Transaction(Running)), "tx2": Tx(Transaction(Running))}
    users: {(age: 10, id: 1), (id: 2, age: 20)}

    States explored: 18

Deadlocks can be avoided by locking the rows we plan on updating first.
One possible options is to use :code:`select ... for update` on these rows:

.. code-block:: entremets

    init do
      `insert into users(id, age) values (1, 10), (2, 20)`
    end

    process do
      transaction tx1 read_committed do
        `select 1 from users where id in (1, 2) for update`
        `update users set age := 11 where id = 1`
        `update users set age := 21 where id = 2`
      end
    end

    process do
      transaction tx2 read_committed do
        `select 1 from users where id in (1, 2) for update`
        `update users set age := 22 where id = 2`
        `update users set age := 12 where id = 1`
      end
    end

This time :code:`entremets` tells us there's no counter example:

.. code-block:: text

    No counter example found
    States explored: 57
