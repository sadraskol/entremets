Features
========

Until all features are implemented, this section will gather existing features.

Entremets statements
--------------------

Transaction
^^^^^^^^^^^

You can open a transaction with the following syntax: :code:`transaction <tx_name> <tx_level> do...`.

* **tx_name:** it can be used as a variable in the spec to check the transaction status
* **tx_level:** for now only :code`read_committed` is supported

Expressions
^^^^^^^^^^^

An expression can be one of:

* An sql expression (see the dedicated section)
* A binary operation (+, -, /, *, %, =, <>, <=, <, in, and, or, >, >=)
* An assignment :code:`<var_name> := <expression>`
* A variable name
* A literal integer or string
* A set :code:`{...}`
* A tuple :code:`(...)`
* A member call, for now only on transactions :code:`<tx_name>.aborted` or :code:`<tx_name>.committed`

Latch
^^^^^

Latches allow processes to wait for each other.
When a process encounters a latch, it wait all other processes to either be waiting for the latch or to have finished.

If/Else
^^^^^^^

Execute the block only if the provided expression is ``true``.
If the expression is ``false``, the optional else block provided in the else is executed instead.


.. code-block:: entremets

    if `update users set age := $t2_age * 2 where id = 1 and age = $t2_age` = 0 do
        abort
    else
    end

Temporal expressions
^^^^^^^^^^^^^^^^^^^^

in properties you can check for the following temporal operators:

* **always:** Checks if the expression provided is ``true`` for every state
* **never:** opposite of always. Checks if the expression provided is ``false`` for every state
* **eventually:** Checks the statement is ``true`` for every possible state path

Sql Expressions
---------------

* **Select:** :code:`select <cols> from <table> where <cond> order_by <order_col> limit <limit> offset <offset> for update`
* **Update:** :code:`update <table> set <col> := <sql_expr> where <cond>`
* **Delete:** :code:`delete from <table> where <cond>`
* **Insert:** :code:`insert into <table>(<cols>) values <tuples>`
* **Unique constraint:** :code:`create unique index on <table>(<cols>)`
* **Foreign keys:** :code:`alter table <table> add constraint <name> foreign key(<cols>) references <foreign_table>(<cols>)`
* Binary operations (+, -, *, /, %, =, and, in, <>, <, <=, >, >=, :code:`between <lower> and <upper>`)
