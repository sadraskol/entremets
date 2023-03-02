Unique Constraint
==================

:code:`entremets` supports unique constraint.
Unique constraint are useful to avoid having duplicate data.

You can declare unique constraint like you would in sql:

.. code-block:: entremets

    init do
      `create unique index on users(id)`
      `create unique index on users(external_id, version)`
    end

