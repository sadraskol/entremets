Getting started
==================

Install `rust <https://www.rust-lang.org/tools/install>`_.
You can then install :code:`entremets` with :code:`cargo install entremets@0.1.0-alpha.4`

Let's write the first code:

.. code-block:: entremets
    :name: no-transaction.mets

    init do
      `insert into users(id, age) values (1, 10)`
    end

    process do
      let age_1 := `select age from users where id = 1`
      `update users set age := $age_1 * 2`
    end

    process do
      let age_2 := `select age from users where id = 1`
      `update users set age := $age_2 + 1`
    end

Run the specification with :code:`entremets no-transaction.mets`.
The output tells us it explored the possible states:

.. code-block:: text

    No counter example found
    States explored: 14

Did you expect this simple program to have 14 states?
Entremets is capable of exploring a lot of states.
But exploring states without asserting properties is boring.
Let's add a simple property:

.. code-block:: entremets
    :name: no-transaction.mets

    init do
      `insert into users(id, age) values (1, 10)`
    end

    process do
      let age_1 := `select age from users where id = 1`
      `update users set age := $age_1 * 2`
    end

    process do
      let age_2 := `select age from users where id = 1`
      `update users set age := $age_2 + 1`
    end

    property eventually(`select age from users where id = 1` in {21, 22})

Either the addition goes first, then the multiplication.
So the :code:`age` should be 21 or 22.
Let's check again with entremets :code:`entremets no-transaction.mets`.
This time the output is a little different:

.. code-block:: text

    Following property was violated: eventually(select age from users where id = 1 in {21, 22})
    The following counter example was found:
    users: {(id: 1, age: 10)}
    Process 0: age_1 := select age from users where id = 1
    Local State {"age_1": Integer(10)}
    users: {(id: 1, age: 10)}
    Process 1: age_2 := select age from users where id = 1
    Local State {"age_1": Integer(10), "age_2": Integer(10)}
    users: {(id: 1, age: 10)}
    Process 0: update users set age := age_1 * 2 where id = 1
    Local State {"age_1": Integer(10), "age_2": Integer(10)}
    users: {(id: 1, age: 20)}
    Process 1: update users set age := age_2 + 1 where id = 1
    Local State {"age_1": Integer(10), "age_2": Integer(10)}
    users: {(id: 1, age: 11)}

    States explored: 12

Entremets output one possible series of event in which the property was violated.
This is a typical case of a race condition.
The first step to fix this in SQL is to use a transaction:

.. code-block:: entremets
    :name: transaction.mets

    init do
      `insert into users(id, age) values (1, 10)`
    end

    process do
      transaction tx1 read_committed do
        let age_1 := `select age from users where id = 1`
        `update users set age := $age_1 * 2`
      end
    end

    process do
      transaction tx2 read_committed do
        let age_2 := `select age from users where id = 1`
        `update users set age := $age_2 + 1`
      end
    end

    property eventually(`select age from users where id = 1` in {21, 22})

But using transaction is not enough.
Entremets can also tell that there's an issue:

.. code-block:: text

    Following property was violated: eventually(select age from users where id = 1 in {21, 22})
    The following counter example was found:
    users: {(age: 10, id: 1)}
    Process 0: begin ReadCommitted (tx1)
    users: {(age: 10, id: 1)}
    Process 0: age_1 := select age from users where id = 1
    Local State {"age_1": Integer(10)}
    users: {(age: 10, id: 1)}
    Process 0: update users set age := age_1 * 2
    Local State {"age_1": Integer(10)}
    users: {(age: 10, id: 1)}
    Process 1: begin ReadCommitted (tx2)
    Local State {"age_1": Integer(10)}
    users: {(age: 10, id: 1)}
    Process 1: age_2 := select age from users where id = 1
    Local State {"age_1": Integer(10), "age_2": Integer(10)}
    users: {(age: 10, id: 1)}
    Process 0: commit
    Local State {"age_1": Integer(10), "age_2": Integer(10)}
    users: {(age: 20, id: 1)}
    Process 1: update users set age := age_2 + 1
    Local State {"age_1": Integer(10), "age_2": Integer(10)}
    users: {(age: 20, id: 1)}
    Process 1: commit
    Local State {"age_1": Integer(10), "age_2": Integer(10)}
    users: {(age: 11, id: 1)}

    States explored: 36

Because we're using read committed isolation, we're not protected against lost updates.
If we want both transaction to complete, we can use manual locking.
SQL offers :code:`select for update` to achieve this:


.. code-block:: entremets
    :name: no-lost-updates.mets

    init do
      `insert into users(id, age) values (1, 10)`
    end

    process do
      transaction tx1 read_committed do
        let age_1 := `select age from users where id = 1 for update`
        `update users set age := $age_1 * 2
      end
    end

    process do
      transaction tx2 read_committed do
        let age_2 := `select age from users where id = 1 for update`
        `update users set age := $age_2 + 1`
      end
    end

    property eventually(`select age from users where id = 1` in {21, 22})

And entremets tells us it cannot find issues with this code:

.. code-block:: text

    No counter example found
    States explored: 22

This was a quick introduction to entremets.